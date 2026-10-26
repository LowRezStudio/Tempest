//! Meshes as Autodesk FBX (binary 7.4): export of a [`Geometry`] with its
//! textured materials and skeleton, and import of an FBX scene into the
//! [`ImportedMesh`] form `mesh_import` builds packages from.
//!
//! Conventions: FBX is Y-up, right-handed; files are written in
//! centimetres (`UnitScaleFactor` 1), so the Unreal `(x, y, z)` becomes
//! `(x, z, y)` and the triangle winding flips. Polygons are triangles,
//! normals and UVs per polygon vertex, materials per polygon. The
//! skeleton is a `Null` root with `LimbNode` models, bound by a `Skin`
//! deformer with one `Cluster` per bone, the way Blender and Maya write
//! it. Textures travel embedded (`Video.Content`, PNG).

use std::collections::HashMap;

use tempest_upk::io::{Error, Result};

use crate::fbx::{self, Node, Prop};
use crate::mesh_import::{compute_normals, ImportedMesh, ImportedSection, ImportedVertex};
use crate::mesh_ops::{mat_inverse_rigid, mat_mul, Geometry};

type Mat = [[f32; 4]; 4];

fn p70(entries: Vec<Node>) -> Node {
    Node { name: "Properties70".into(), props: Vec::new(), children: entries }
}

fn p(name: &str, ty: &str, label: &str, flags: &str, values: Vec<Prop>) -> Node {
    let mut props = vec![Prop::Str(name.into()), Prop::Str(ty.into()), Prop::Str(label.into()), Prop::Str(flags.into())];
    props.extend(values);
    Node::with("P", props)
}

fn p_vec(name: &str, ty: &str, v: [f64; 3]) -> Node {
    p(name, ty, "", "A", vec![Prop::F64(v[0]), Prop::F64(v[1]), Prop::F64(v[2])])
}

fn obj_name(name: &str, class: &str) -> String {
    format!("{name}\0\u{1}{class}")
}

fn one(name: &str, v: Prop) -> Node {
    Node::with(name, vec![v])
}

/// Unreal → FBX axes (centimetres kept).
fn to_fbx(v: [f32; 3]) -> [f64; 3] {
    [v[0] as f64, v[2] as f64, v[1] as f64]
}

/// `C M C⁻¹` with the y/z swap (no scaling), column-major.
fn swap_matrix(m: &Mat) -> Mat {
    let q = |i: usize| match i {
        1 => 2,
        2 => 1,
        i => i,
    };
    let mut out = [[0f32; 4]; 4];
    for c in 0..3 {
        for r in 0..3 {
            out[c][r] = m[q(c)][q(r)];
        }
    }
    for r in 0..3 {
        out[3][r] = m[3][q(r)];
    }
    out[3][3] = 1.0;
    out
}

fn flat(m: &Mat) -> Prop {
    Prop::F64s(m.iter().flatten().map(|x| *x as f64).collect())
}

/// Euler XYZ angles in degrees of a column-major rotation (`R = Rz·Ry·Rx`).
fn euler_xyz_deg(m: &Mat) -> [f64; 3] {
    let r = |row: usize, col: usize| m[col][row] as f64;
    let sy = (-r(2, 0)).clamp(-1.0, 1.0);
    let y = sy.asin();
    let (x, z) = if y.cos().abs() > 1e-6 { (r(2, 1).atan2(r(2, 2)), r(1, 0).atan2(r(0, 0))) } else { ((-r(1, 2)).atan2(r(1, 1)), 0.0) };
    [x.to_degrees(), y.to_degrees(), z.to_degrees()]
}

/// Column-major rotation from Euler XYZ degrees (`R = Rz·Ry·Rx`).
fn rotation_xyz(deg: [f64; 3]) -> Mat {
    let (sx, cx) = deg[0].to_radians().sin_cos();
    let (sy, cy) = deg[1].to_radians().sin_cos();
    let (sz, cz) = deg[2].to_radians().sin_cos();
    let row = |r: usize, c: usize| -> f64 {
        match (r, c) {
            (0, 0) => cz * cy,
            (0, 1) => cz * sy * sx - sz * cx,
            (0, 2) => cz * sy * cx + sz * sx,
            (1, 0) => sz * cy,
            (1, 1) => sz * sy * sx + cz * cx,
            (1, 2) => sz * sy * cx - cz * sx,
            (2, 0) => -sy,
            (2, 1) => cy * sx,
            (2, 2) => cy * cx,
            _ => 0.0,
        }
    };
    let mut m = [[0f32; 4]; 4];
    for (c, col) in m.iter_mut().enumerate().take(3) {
        for (r, cell) in col.iter_mut().enumerate().take(3) {
            *cell = row(r, c) as f32;
        }
    }
    m[3][3] = 1.0;
    m
}

// ----------------------------------------------------------------------------
// Export
// ----------------------------------------------------------------------------

fn material_node(id: i64, name: &str) -> Node {
    Node::with("Material", vec![Prop::I64(id), Prop::Str(obj_name(name, "Material")), Prop::Str(String::new())])
        .with_child(one("Version", Prop::I32(102)))
        .with_child(one("ShadingModel", Prop::Str("Phong".into())))
        .with_child(one("MultiLayer", Prop::I32(0)))
        .with_child(p70(vec![
            p_vec("DiffuseColor", "Color", [0.8, 0.8, 0.8]),
            p_vec("AmbientColor", "Color", [0.0, 0.0, 0.0]),
            p_vec("SpecularColor", "Color", [0.2, 0.2, 0.2]),
            p_vec("EmissiveColor", "Color", [0.0, 0.0, 0.0]),
            p("ShininessExponent", "Number", "", "A", vec![Prop::F64(20.0)]),
            p("Opacity", "Number", "", "A", vec![Prop::F64(1.0)]),
        ]))
}

fn video_node(id: i64, name: &str, file: &str, png: &[u8]) -> Node {
    Node::with("Video", vec![Prop::I64(id), Prop::Str(obj_name(name, "Video")), Prop::Str("Clip".into())])
        .with_child(one("Type", Prop::Str("Clip".into())))
        .with_child(p70(vec![p("Path", "KString", "XRefUrl", "", vec![Prop::Str(file.into())])]))
        .with_child(one("UseMipMap", Prop::I32(0)))
        .with_child(one("Filename", Prop::Str(file.into())))
        .with_child(one("RelativeFilename", Prop::Str(file.into())))
        .with_child(one("Content", Prop::Raw(png.to_vec())))
}

fn texture_node(id: i64, name: &str, file: &str) -> Node {
    Node::with("Texture", vec![Prop::I64(id), Prop::Str(obj_name(name, "Texture")), Prop::Str(String::new())])
        .with_child(one("Type", Prop::Str("TextureVideoClip".into())))
        .with_child(one("Version", Prop::I32(202)))
        .with_child(one("TextureName", Prop::Str(obj_name(name, "Texture"))))
        .with_child(p70(vec![p("UVSet", "KString", "", "", vec![Prop::Str("UVMap".into())]), p("UseMaterial", "bool", "", "", vec![Prop::I32(1)])]))
        .with_child(one("Media", Prop::Str(obj_name(name, "Video"))))
        .with_child(one("FileName", Prop::Str(file.into())))
        .with_child(one("RelativeFilename", Prop::Str(file.into())))
        .with_child(Node::with("ModelUVTranslation", vec![Prop::F64(0.0), Prop::F64(0.0)]))
        .with_child(Node::with("ModelUVScaling", vec![Prop::F64(1.0), Prop::F64(1.0)]))
        .with_child(one("Texture_Alpha_Source", Prop::Str("None".into())))
        .with_child(Node::with("Cropping", vec![Prop::I32(0), Prop::I32(0), Prop::I32(0), Prop::I32(0)]))
}

fn limb_node(id: i64, name: &str, local: &Mat) -> Node {
    let t = [local[3][0] as f64, local[3][1] as f64, local[3][2] as f64];
    let r = euler_xyz_deg(local);
    Node::with("Model", vec![Prop::I64(id), Prop::Str(obj_name(name, "Model")), Prop::Str("LimbNode".into())])
        .with_child(one("Version", Prop::I32(232)))
        .with_child(p70(vec![p_vec("Lcl Translation", "Lcl Translation", t), p_vec("Lcl Rotation", "Lcl Rotation", r), p_vec("Lcl Scaling", "Lcl Scaling", [1.0, 1.0, 1.0]), p("RotationActive", "bool", "", "", vec![Prop::I32(1)]), p("InheritType", "enum", "", "", vec![Prop::I32(1)])]))
        .with_child(one("Shading", Prop::Bool(true)))
        .with_child(one("Culling", Prop::Str("CullingOff".into())))
}

fn conn_oo(connections: &mut Node, child: i64, parent: i64) {
    connections.children.push(Node::with("C", vec![Prop::Str("OO".into()), Prop::I64(child), Prop::I64(parent)]));
}

fn conn_op(connections: &mut Node, child: i64, parent: i64, prop: &str) {
    connections.children.push(Node::with("C", vec![Prop::Str("OP".into()), Prop::I64(child), Prop::I64(parent), Prop::Str(prop.into())]));
}

/// Binary FBX of a mesh: one `Mesh` model (materials, embedded textures)
/// and, when `g.bones` is filled, its skeleton and skin.
pub fn write_fbx(name: &str, g: &Geometry) -> Result<Vec<u8>> {
    let mut next = 100_000i64;
    let mut id = move || {
        next += 1;
        next
    };
    let n = g.positions.len();
    let skinned = !g.bones.is_empty() && g.influences.len() == n;

    // Geometry.
    let mut vertices = Vec::with_capacity(n * 3);
    for v in &g.positions {
        vertices.extend_from_slice(&to_fbx(*v));
    }
    let mut poly = Vec::new();
    let mut normals = Vec::new();
    let mut uv_index = Vec::new();
    let mut poly_material = Vec::new();
    let mut material_names: Vec<String> = Vec::new();
    for (si, sec) in g.sections.iter().enumerate() {
        let start = sec.first_index as usize;
        let count = sec.num_triangles as usize * 3;
        let slice = g.indices.get(start..start + count).ok_or_else(|| Error::Format("section exceeds index buffer".into()))?;
        let mname = g.materials.get(si).map(|m| m.name.clone()).unwrap_or_else(|| sec.material_name.clone());
        let mi = match material_names.iter().position(|m| *m == mname) {
            Some(i) => i,
            None => {
                material_names.push(mname);
                material_names.len() - 1
            }
        };
        for t in slice.chunks_exact(3) {
            // Flip winding: the axis swap mirrors the mesh.
            for (k, &i) in [t[0], t[2], t[1]].iter().enumerate() {
                poly.push(if k == 2 { !(i as i32) } else { i as i32 });
                uv_index.push(i as i32);
                let nm = g.normals.get(i as usize).copied().unwrap_or([0.0, 0.0, 1.0]);
                normals.extend_from_slice(&to_fbx(nm));
            }
            poly_material.push(mi as i32);
        }
    }
    let mut uvs = Vec::with_capacity(n * 2);
    for uv in &g.uvs {
        uvs.push(uv[0] as f64);
        uvs.push(1.0 - uv[1] as f64);
    }
    let geom_id = id();
    let geometry = Node::with("Geometry", vec![Prop::I64(geom_id), Prop::Str(obj_name(name, "Geometry")), Prop::Str("Mesh".into())])
        .with_child(p70(vec![]))
        .with_child(one("GeometryVersion", Prop::I32(124)))
        .with_child(one("Vertices", Prop::F64s(vertices)))
        .with_child(one("PolygonVertexIndex", Prop::I32s(poly)))
        .with_child(
            Node::with("LayerElementNormal", vec![Prop::I32(0)])
                .with_child(one("Version", Prop::I32(101)))
                .with_child(one("Name", Prop::Str(String::new())))
                .with_child(one("MappingInformationType", Prop::Str("ByPolygonVertex".into())))
                .with_child(one("ReferenceInformationType", Prop::Str("Direct".into())))
                .with_child(one("Normals", Prop::F64s(normals))),
        )
        .with_child(
            Node::with("LayerElementUV", vec![Prop::I32(0)])
                .with_child(one("Version", Prop::I32(101)))
                .with_child(one("Name", Prop::Str("UVMap".into())))
                .with_child(one("MappingInformationType", Prop::Str("ByPolygonVertex".into())))
                .with_child(one("ReferenceInformationType", Prop::Str("IndexToDirect".into())))
                .with_child(one("UV", Prop::F64s(uvs)))
                .with_child(one("UVIndex", Prop::I32s(uv_index))),
        )
        .with_child(
            Node::with("LayerElementMaterial", vec![Prop::I32(0)])
                .with_child(one("Version", Prop::I32(101)))
                .with_child(one("Name", Prop::Str(String::new())))
                .with_child(one("MappingInformationType", Prop::Str("ByPolygon".into())))
                .with_child(one("ReferenceInformationType", Prop::Str("IndexToDirect".into())))
                .with_child(one("Materials", Prop::I32s(poly_material))),
        )
        .with_child(
            Node::with("Layer", vec![Prop::I32(0)])
                .with_child(one("Version", Prop::I32(100)))
                .with_child(Node::new("LayerElement").with_child(one("Type", Prop::Str("LayerElementNormal".into()))).with_child(one("TypedIndex", Prop::I32(0))))
                .with_child(Node::new("LayerElement").with_child(one("Type", Prop::Str("LayerElementMaterial".into()))).with_child(one("TypedIndex", Prop::I32(0))))
                .with_child(Node::new("LayerElement").with_child(one("Type", Prop::Str("LayerElementUV".into()))).with_child(one("TypedIndex", Prop::I32(0)))),
        );

    let model_id = id();
    let model = Node::with("Model", vec![Prop::I64(model_id), Prop::Str(obj_name(name, "Model")), Prop::Str("Mesh".into())])
        .with_child(one("Version", Prop::I32(232)))
        .with_child(p70(vec![p("DefaultAttributeIndex", "int", "Integer", "", vec![Prop::I32(0)]), p("InheritType", "enum", "", "", vec![Prop::I32(1)])]))
        .with_child(one("Shading", Prop::Bool(true)))
        .with_child(one("Culling", Prop::Str("CullingOff".into())));

    let mut objects = Node::new("Objects").with_child(geometry).with_child(model);
    let mut connections = Node::new("Connections");
    conn_oo(&mut connections, model_id, 0);
    conn_oo(&mut connections, geom_id, model_id);

    // Materials and their textures (one Video + Texture per image).
    let mut image_ids: HashMap<usize, i64> = HashMap::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for mname in &material_names {
        let mat_id = id();
        objects.children.push(material_node(mat_id, mname));
        conn_oo(&mut connections, mat_id, model_id);
        *counts.entry("Material").or_default() += 1;
        let Some(m) = g.materials.iter().find(|m| m.name == *mname) else { continue };
        for (slot, image) in [("DiffuseColor", m.base_color), ("NormalMap", m.normal), ("EmissiveColor", m.emissive)] {
            let Some(img) = image else { continue };
            let Some(image) = g.images.get(img) else { continue };
            let tex_id = match image_ids.get(&img) {
                Some(&t) => t,
                None => {
                    let file = format!("{}.png", image.name.replace(['/', '\\', ':'], "_"));
                    let video_id = id();
                    let tex_id = id();
                    objects.children.push(video_node(video_id, &image.name, &file, &image.png));
                    objects.children.push(texture_node(tex_id, &image.name, &file));
                    conn_oo(&mut connections, video_id, tex_id);
                    image_ids.insert(img, tex_id);
                    *counts.entry("Video").or_default() += 1;
                    *counts.entry("Texture").or_default() += 1;
                    tex_id
                }
            };
            conn_op(&mut connections, tex_id, mat_id, slot);
        }
    }

    // Skeleton and skin.
    let mut pose_nodes: Vec<(i64, Mat)> = Vec::new();
    if skinned {
        let root_id = id();
        let root_attr = id();
        objects.children.push(
            Node::with("NodeAttribute", vec![Prop::I64(root_attr), Prop::Str(obj_name(&format!("{name}_skeleton"), "NodeAttribute")), Prop::Str("Null".into())])
                .with_child(one("TypeFlags", Prop::Str("Null".into()))),
        );
        objects.children.push(
            Node::with("Model", vec![Prop::I64(root_id), Prop::Str(obj_name(&format!("{name}_skeleton"), "Model")), Prop::Str("Null".into())])
                .with_child(one("Version", Prop::I32(232)))
                .with_child(p70(vec![]))
                .with_child(one("Shading", Prop::Bool(true)))
                .with_child(one("Culling", Prop::Str("CullingOff".into()))),
        );
        conn_oo(&mut connections, root_attr, root_id);
        conn_oo(&mut connections, root_id, 0);
        *counts.entry("Model").or_default() += 1;
        *counts.entry("NodeAttribute").or_default() += 1;

        let mut bone_ids = Vec::with_capacity(g.bones.len());
        let mut globals: Vec<Mat> = Vec::with_capacity(g.bones.len());
        for (i, b) in g.bones.iter().enumerate() {
            let local = swap_matrix(&b.local);
            let global = match b.parent {
                Some(p) if p < i => mat_mul(&globals[p], &local),
                _ => local,
            };
            globals.push(global);
            let bone_id = id();
            let attr_id = id();
            bone_ids.push(bone_id);
            objects.children.push(
                Node::with("NodeAttribute", vec![Prop::I64(attr_id), Prop::Str(obj_name(&b.name, "NodeAttribute")), Prop::Str("LimbNode".into())])
                    .with_child(p70(vec![p("Size", "double", "Number", "", vec![Prop::F64(1.0)])]))
                    .with_child(one("TypeFlags", Prop::Str("Skeleton".into()))),
            );
            objects.children.push(limb_node(bone_id, &b.name, &local));
            conn_oo(&mut connections, attr_id, bone_id);
            let parent_id = match b.parent {
                Some(p) if p < i => bone_ids[p],
                _ => root_id,
            };
            conn_oo(&mut connections, bone_id, parent_id);
            *counts.entry("Model").or_default() += 1;
            *counts.entry("NodeAttribute").or_default() += 1;
        }

        let skin_id = id();
        objects.children.push(
            Node::with("Deformer", vec![Prop::I64(skin_id), Prop::Str(obj_name("", "Deformer")), Prop::Str("Skin".into())])
                .with_child(one("Version", Prop::I32(101)))
                .with_child(one("Link_DeformAcuracy", Prop::F64(50.0)))
                .with_child(one("SkinningType", Prop::Str("Linear".into()))),
        );
        conn_oo(&mut connections, skin_id, geom_id);
        *counts.entry("Deformer").or_default() += 1;
        let mut per_bone: Vec<(Vec<i32>, Vec<f64>)> = vec![(Vec::new(), Vec::new()); g.bones.len()];
        for (vi, inf) in g.influences.iter().enumerate() {
            let total: u32 = inf.iter().map(|(_, w)| *w as u32).sum();
            if total == 0 {
                continue;
            }
            for (b, w) in inf {
                if *w == 0 || (*b as usize) >= g.bones.len() {
                    continue;
                }
                let e = &mut per_bone[*b as usize];
                e.0.push(vi as i32);
                e.1.push(*w as f64 / total as f64);
            }
        }
        let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        pose_nodes.push((model_id, identity));
        for (bi, (idx, w)) in per_bone.iter().enumerate() {
            pose_nodes.push((bone_ids[bi], globals[bi]));
            if idx.is_empty() {
                continue;
            }
            let cluster_id = id();
            objects.children.push(
                Node::with("Deformer", vec![Prop::I64(cluster_id), Prop::Str(obj_name(&g.bones[bi].name, "SubDeformer")), Prop::Str("Cluster".into())])
                    .with_child(one("Version", Prop::I32(100)))
                    .with_child(Node::with("UserData", vec![Prop::Str(String::new()), Prop::Str(String::new())]))
                    .with_child(one("Indexes", Prop::I32s(idx.clone())))
                    .with_child(one("Weights", Prop::F64s(w.clone())))
                    .with_child(one("Transform", flat(&mat_inverse_rigid(&globals[bi]))))
                    .with_child(one("TransformLink", flat(&globals[bi]))),
            );
            conn_oo(&mut connections, cluster_id, skin_id);
            conn_oo(&mut connections, bone_ids[bi], cluster_id);
            *counts.entry("Deformer").or_default() += 1;
        }
        let pose_id = id();
        let mut pose = Node::with("Pose", vec![Prop::I64(pose_id), Prop::Str(obj_name("", "Pose")), Prop::Str("BindPose".into())])
            .with_child(one("Type", Prop::Str("BindPose".into())))
            .with_child(one("Version", Prop::I32(100)))
            .with_child(one("NbPoseNodes", Prop::I32(pose_nodes.len() as i32)));
        for (node_id, m) in &pose_nodes {
            pose.children.push(Node::new("PoseNode").with_child(one("Node", Prop::I64(*node_id))).with_child(one("Matrix", flat(m))));
        }
        objects.children.push(pose);
        *counts.entry("Pose").or_default() += 1;
    }
    *counts.entry("Model").or_default() += 1;
    *counts.entry("Geometry").or_default() += 1;

    // Header, settings, definitions.
    let header = Node::new("FBXHeaderExtension")
        .with_child(one("FBXHeaderVersion", Prop::I32(1003)))
        .with_child(one("FBXVersion", Prop::I32(7400)))
        .with_child(one("EncryptionType", Prop::I32(0)))
        .with_child(
            Node::new("CreationTimeStamp")
                .with_child(one("Version", Prop::I32(1000)))
                .with_child(one("Year", Prop::I32(2026)))
                .with_child(one("Month", Prop::I32(1)))
                .with_child(one("Day", Prop::I32(1)))
                .with_child(one("Hour", Prop::I32(0)))
                .with_child(one("Minute", Prop::I32(0)))
                .with_child(one("Second", Prop::I32(0)))
                .with_child(one("Millisecond", Prop::I32(0))),
        )
        .with_child(one("Creator", Prop::Str("tempest-assets".into())))
        .with_child(
            Node::with("SceneInfo", vec![Prop::Str(obj_name("GlobalInfo", "SceneInfo")), Prop::Str("UserData".into())])
                .with_child(one("Type", Prop::Str("UserData".into())))
                .with_child(one("Version", Prop::I32(100)))
                .with_child(Node::new("MetaData").with_child(one("Version", Prop::I32(100))).with_child(one("Title", Prop::Str(String::new()))).with_child(one("Subject", Prop::Str(String::new()))).with_child(one("Author", Prop::Str(String::new()))).with_child(one("Keywords", Prop::Str(String::new()))).with_child(one("Revision", Prop::Str(String::new()))).with_child(one("Comment", Prop::Str(String::new()))))
                .with_child(p70(vec![p("DocumentUrl", "KString", "Url", "", vec![Prop::Str(format!("{name}.fbx"))]), p("Original|ApplicationName", "KString", "", "", vec![Prop::Str("tempest-assets".into())]), p("LastSaved|ApplicationName", "KString", "", "", vec![Prop::Str("tempest-assets".into())])])),
        );
    let global_settings = Node::new("GlobalSettings").with_child(one("Version", Prop::I32(1000))).with_child(p70(vec![
        p("UpAxis", "int", "Integer", "", vec![Prop::I32(1)]),
        p("UpAxisSign", "int", "Integer", "", vec![Prop::I32(1)]),
        p("FrontAxis", "int", "Integer", "", vec![Prop::I32(2)]),
        p("FrontAxisSign", "int", "Integer", "", vec![Prop::I32(1)]),
        p("CoordAxis", "int", "Integer", "", vec![Prop::I32(0)]),
        p("CoordAxisSign", "int", "Integer", "", vec![Prop::I32(1)]),
        p("OriginalUpAxis", "int", "Integer", "", vec![Prop::I32(-1)]),
        p("OriginalUpAxisSign", "int", "Integer", "", vec![Prop::I32(1)]),
        p("UnitScaleFactor", "double", "Number", "", vec![Prop::F64(1.0)]),
        p("OriginalUnitScaleFactor", "double", "Number", "", vec![Prop::F64(1.0)]),
        p("AmbientColor", "ColorRGB", "Color", "", vec![Prop::F64(0.0), Prop::F64(0.0), Prop::F64(0.0)]),
        p("DefaultCamera", "KString", "", "", vec![Prop::Str("Producer Perspective".into())]),
        p("TimeMode", "enum", "", "", vec![Prop::I32(11)]),
        p("TimeSpanStart", "KTime", "Time", "", vec![Prop::I64(0)]),
        p("TimeSpanStop", "KTime", "Time", "", vec![Prop::I64(46186158000)]),
        p("CustomFrameRate", "double", "Number", "", vec![Prop::F64(24.0)]),
    ]));
    let documents = Node::new("Documents").with_child(one("Count", Prop::I32(1))).with_child(
        Node::with("Document", vec![Prop::I64(id()), Prop::Str(String::new()), Prop::Str("Scene".into())])
            .with_child(p70(vec![p("SourceObject", "object", "", "", vec![]), p("ActiveAnimStackName", "KString", "", "", vec![Prop::Str(String::new())])]))
            .with_child(one("RootNode", Prop::I64(0))),
    );
    let mut definitions = Node::new("Definitions").with_child(one("Version", Prop::I32(100))).with_child(one("Count", Prop::I32(counts.values().sum::<usize>() as i32 + 1)));
    definitions.children.push(Node::with("ObjectType", vec![Prop::Str("GlobalSettings".into())]).with_child(one("Count", Prop::I32(1))));
    for (ty, template) in [("Model", "FbxNode"), ("Geometry", "FbxMesh"), ("Material", "FbxSurfacePhong"), ("Texture", "FbxFileTexture"), ("Video", "FbxVideo"), ("NodeAttribute", "FbxNull"), ("Deformer", ""), ("Pose", "")] {
        let Some(&c) = counts.get(ty) else { continue };
        let mut ot = Node::with("ObjectType", vec![Prop::Str(ty.into())]).with_child(one("Count", Prop::I32(c as i32)));
        if !template.is_empty() {
            ot.children.push(Node::with("PropertyTemplate", vec![Prop::Str(template.into())]).with_child(p70(vec![])));
        }
        definitions.children.push(ot);
    }
    let takes = Node::new("Takes").with_child(one("Current", Prop::Str(String::new())));
    let nodes = vec![
        header,
        one("FileId", Prop::Raw(vec![0x28, 0xb3, 0x2a, 0xeb, 0xb6, 0x24, 0xcc, 0xc2, 0xbf, 0xc8, 0xb0, 0x2a, 0xa9, 0x2b, 0xfc, 0xf1])),
        one("CreationTime", Prop::Str("2026-01-01 00:00:00:000".into())),
        one("Creator", Prop::Str("tempest-assets".into())),
        global_settings,
        documents,
        Node::new("References"),
        definitions,
        objects,
        connections,
        takes,
    ];
    Ok(fbx::write(7400, &nodes))
}

// ----------------------------------------------------------------------------
// Import
// ----------------------------------------------------------------------------

/// `"name\0\u{1}Class"` → `name` (a `Namespace:name` prefix dropped).
fn plain_name(s: &str) -> String {
    let n = s.split("\0\u{1}").next().unwrap_or(s);
    n.rsplit("::").next().unwrap_or(n).rsplit(':').next().unwrap_or(n).to_string()
}

struct Scene<'a> {
    objects: HashMap<i64, &'a Node>,
    /// `(child, parent, property)`.
    connections: Vec<(i64, i64, Option<String>)>,
}

impl<'a> Scene<'a> {
    fn children(&self, parent: i64, kind: &str) -> Vec<&'a Node> {
        self.connections.iter().filter(|(_, p, _)| *p == parent).filter_map(|(c, _, _)| self.objects.get(c).copied()).filter(|n| n.name == kind).collect()
    }
    fn parents(&self, child: i64, kind: &str) -> Vec<&'a Node> {
        self.connections.iter().filter(|(c, _, _)| *c == child).filter_map(|(_, p, _)| self.objects.get(p).copied()).filter(|n| n.name == kind).collect()
    }
}

fn vec3(n: &Node, name: &str, default: [f64; 3]) -> [f64; 3] {
    match n.property(name) {
        Some(p) => [p.f64(4).unwrap_or(default[0]), p.f64(5).unwrap_or(default[1]), p.f64(6).unwrap_or(default[2])],
        None => default,
    }
}

/// Local transform of a model: `T · Rpre · R · S` (pivots and offsets,
/// which exporters leave at zero, are ignored).
fn local_matrix(model: &Node) -> Mat {
    let t = vec3(model, "Lcl Translation", [0.0; 3]);
    let r = vec3(model, "Lcl Rotation", [0.0; 3]);
    let pre = vec3(model, "PreRotation", [0.0; 3]);
    let s = vec3(model, "Lcl Scaling", [1.0; 3]);
    let mut m = mat_mul(&rotation_xyz(pre), &rotation_xyz(r));
    for (c, col) in m.iter_mut().enumerate().take(3) {
        for cell in col.iter_mut().take(3) {
            *cell *= s[c] as f32;
        }
    }
    m[3] = [t[0] as f32, t[1] as f32, t[2] as f32, 1.0];
    m
}

fn world_matrix(scene: &Scene, model: &Node, depth: usize) -> Mat {
    let local = local_matrix(model);
    if depth > 64 {
        return local;
    }
    let id = model.i64(0).unwrap_or(0);
    match scene.parents(id, "Model").first() {
        Some(parent) => mat_mul(&world_matrix(scene, parent, depth + 1), &local),
        None => local,
    }
}

fn transform_point(m: &Mat, p: [f64; 3]) -> [f64; 3] {
    let mut out = [0.0; 3];
    for (r, o) in out.iter_mut().enumerate() {
        *o = m[0][r] as f64 * p[0] + m[1][r] as f64 * p[1] + m[2][r] as f64 * p[2] + m[3][r] as f64;
    }
    out
}

fn transform_dir(m: &Mat, p: [f64; 3]) -> [f64; 3] {
    let mut out = [0.0; 3];
    for (r, o) in out.iter_mut().enumerate() {
        *o = m[0][r] as f64 * p[0] + m[1][r] as f64 * p[1] + m[2][r] as f64 * p[2];
    }
    out
}

/// Welding key of an imported vertex: control point, normal bits, UV bits.
type WeldKey = (usize, [u32; 3], Vec<[u32; 2]>);

/// A layer element's value for one polygon vertex, by its mapping.
struct Layer {
    data: Vec<f64>,
    width: usize,
    index: Option<Vec<i64>>,
    mapping: String,
}

impl Layer {
    fn read(n: &Node, data_name: &str, index_name: &str, width: usize) -> Option<Layer> {
        let data = n.child(data_name)?.f64s(0)?;
        let index = n.child(index_name).and_then(|c| c.i64s(0));
        let mapping = n.child_str("MappingInformationType").unwrap_or("ByPolygonVertex").to_string();
        let reference = n.child_str("ReferenceInformationType").unwrap_or("Direct");
        let index = if reference == "IndexToDirect" { index } else { None };
        Some(Layer { data, width, index, mapping })
    }

    fn get(&self, poly_vertex: usize, control_point: usize, polygon: usize) -> Option<&[f64]> {
        let k = match self.mapping.as_str() {
            "ByPolygonVertex" => poly_vertex,
            "ByVertice" | "ByVertex" => control_point,
            "ByPolygon" => polygon,
            "AllSame" => 0,
            _ => poly_vertex,
        };
        let k = match &self.index {
            Some(idx) => {
                let v = *idx.get(k)?;
                if v < 0 {
                    return None;
                }
                v as usize
            }
            None => k,
        };
        self.data.get(k * self.width..(k + 1) * self.width)
    }
}

/// Parse a binary FBX into the import form: every `Mesh` model's
/// geometry, materials by name, and the skin of the first skinned mesh.
pub fn parse_fbx(bytes: &[u8]) -> Result<ImportedMesh> {
    let (_, nodes) = fbx::parse(bytes)?;
    let top = |name: &str| nodes.iter().find(|n| n.name == name);
    let settings = top("GlobalSettings");
    let unit = settings.and_then(|g| g.property("UnitScaleFactor")).and_then(|p| p.f64(4)).unwrap_or(1.0);
    let up_axis = settings.and_then(|g| g.property("UpAxis")).and_then(|p| p.i64(4)).unwrap_or(1);
    let objects = top("Objects").ok_or_else(|| Error::Format("FBX: no Objects".into()))?;
    let mut scene = Scene { objects: HashMap::new(), connections: Vec::new() };
    for o in &objects.children {
        if let Some(id) = o.i64(0) {
            scene.objects.insert(id, o);
        }
    }
    if let Some(c) = top("Connections") {
        for conn in c.children_named("C") {
            if let (Some(child), Some(parent)) = (conn.i64(1), conn.i64(2)) {
                scene.connections.push((child, parent, conn.str(3).map(str::to_string)));
            }
        }
    }
    // FBX (Y-up unless the file says Z-up) → Unreal: (x, z, y) in centimetres.
    let to_engine = |p: [f64; 3]| -> [f32; 3] {
        let y_up = if up_axis == 2 { [p[0], p[2], -p[1]] } else { p };
        [(y_up[0] * unit) as f32, (y_up[2] * unit) as f32, (y_up[1] * unit) as f32]
    };
    let to_engine_dir = |p: [f64; 3]| -> [f32; 3] {
        let y_up = if up_axis == 2 { [p[0], p[2], -p[1]] } else { p };
        let v = [y_up[0] as f32, y_up[2] as f32, y_up[1] as f32];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len > 1e-8 { [v[0] / len, v[1] / len, v[2] / len] } else { [0.0, 0.0, 0.0] }
    };

    let mut mesh = ImportedMesh::default();
    let mut joint_index: HashMap<String, u16> = HashMap::new();
    for model in objects.children.iter().filter(|n| n.name == "Model" && n.str(2) == Some("Mesh")) {
        let model_id = model.i64(0).unwrap_or(0);
        let Some(geom) = scene.children(model_id, "Geometry").into_iter().find(|g| g.str(2) == Some("Mesh")) else { continue };
        let geom_id = geom.i64(0).unwrap_or(0);
        let world = world_matrix(&scene, model, 0);
        let Some(verts) = geom.child("Vertices").and_then(|v| v.f64s(0)) else { continue };
        let control_points: Vec<[f64; 3]> = verts.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
        let Some(poly_index) = geom.child("PolygonVertexIndex").and_then(|v| v.i64s(0)) else { continue };
        // Polygons as runs of polygon-vertex positions.
        let mut polygons: Vec<Vec<usize>> = Vec::new();
        let mut cur = Vec::new();
        for (k, &raw) in poly_index.iter().enumerate() {
            cur.push(k);
            if raw < 0 {
                polygons.push(std::mem::take(&mut cur));
            }
        }
        if !cur.is_empty() {
            polygons.push(cur);
        }
        let cp_of = |k: usize| -> usize {
            let raw = poly_index[k];
            if raw < 0 { (!raw) as usize } else { raw as usize }
        };
        let normals = geom.child("LayerElementNormal").and_then(|l| Layer::read(l, "Normals", "NormalsIndex", 3));
        let uv_layers: Vec<Layer> = geom.children_named("LayerElementUV").filter_map(|l| Layer::read(l, "UV", "UVIndex", 2)).collect();
        let materials = geom.child("LayerElementMaterial").and_then(|l| {
            let data = l.child("Materials")?.i64s(0)?;
            let mapping = l.child_str("MappingInformationType").unwrap_or("AllSame").to_string();
            Some((data, mapping))
        });
        let material_names: Vec<String> = scene.children(model_id, "Material").iter().map(|m| plain_name(m.str(1).unwrap_or(""))).collect();

        // Skin: clusters of the skin deformer bound to this geometry.
        let mut cp_weights: Vec<Vec<(u16, f32)>> = vec![Vec::new(); control_points.len()];
        let mut skinned = false;
        for skin in scene.children(geom_id, "Deformer").into_iter().filter(|d| d.str(2) == Some("Skin")) {
            let skin_id = skin.i64(0).unwrap_or(0);
            for cluster in scene.children(skin_id, "Deformer").into_iter().filter(|d| d.str(2) == Some("Cluster")) {
                let cluster_id = cluster.i64(0).unwrap_or(0);
                let Some(bone) = scene.children(cluster_id, "Model").first().copied() else { continue };
                let bone_name = plain_name(bone.str(1).unwrap_or(""));
                let j = match joint_index.get(&bone_name) {
                    Some(&j) => j,
                    None => {
                        let j = mesh.joint_names.len() as u16;
                        mesh.joint_names.push(bone_name.clone());
                        joint_index.insert(bone_name, j);
                        j
                    }
                };
                let idx = cluster.child("Indexes").and_then(|c| c.i64s(0)).unwrap_or_default();
                let w = cluster.child("Weights").and_then(|c| c.f64s(0)).unwrap_or_default();
                for (i, wt) in idx.iter().zip(&w) {
                    if let Some(list) = cp_weights.get_mut(*i as usize) {
                        if *wt > 0.0 {
                            list.push((j, *wt as f32));
                        }
                    }
                }
                skinned = true;
            }
        }

        // Sections per material; vertices welded per (control point, normal, uv).
        let mut sections: Vec<(i64, ImportedSection, HashMap<WeldKey, u32>)> = Vec::new();
        for (pi, poly) in polygons.iter().enumerate() {
            if poly.len() < 3 {
                continue;
            }
            let mat = match &materials {
                Some((data, mapping)) => match mapping.as_str() {
                    "ByPolygon" => data.get(pi).copied().unwrap_or(0),
                    _ => data.first().copied().unwrap_or(0),
                },
                None => 0,
            };
            let si = match sections.iter().position(|(m, _, _)| *m == mat) {
                Some(i) => i,
                None => {
                    let name = material_names.get(mat.max(0) as usize).cloned().unwrap_or_default();
                    sections.push((mat, ImportedSection { material: name, ..Default::default() }, HashMap::new()));
                    sections.len() - 1
                }
            };
            let (_, sec, weld) = &mut sections[si];
            let mut emitted: Vec<u32> = Vec::with_capacity(poly.len());
            for &pv in poly {
                let cp = cp_of(pv);
                let Some(&cp_pos) = control_points.get(cp) else { continue };
                let n = normals.as_ref().and_then(|l| l.get(pv, cp, pi)).map(|v| [v[0], v[1], v[2]]);
                let uvs: Vec<[f64; 2]> = uv_layers.iter().filter_map(|l| l.get(pv, cp, pi)).map(|v| [v[0], 1.0 - v[1]]).collect();
                let key = (cp, n.map(|v| [v[0].to_bits() as u32, v[1].to_bits() as u32, v[2].to_bits() as u32]).unwrap_or([0; 3]), uvs.iter().map(|u| [u[0].to_bits() as u32, u[1].to_bits() as u32]).collect::<Vec<_>>());
                let vi = match weld.get(&key) {
                    Some(&vi) => vi,
                    None => {
                        let mut v = ImportedVertex { position: to_engine(transform_point(&world, cp_pos)), normal: n.map(|n| to_engine_dir(transform_dir(&world, n))).unwrap_or([0.0; 3]), uvs: uvs.iter().map(|u| [u[0] as f32, u[1] as f32]).collect(), ..Default::default() };
                        if v.uvs.is_empty() {
                            v.uvs.push([0.0, 0.0]);
                        }
                        let mut inf = cp_weights[cp].clone();
                        inf.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                        inf.truncate(4);
                        let total: f32 = inf.iter().map(|(_, w)| *w).sum();
                        for (k, (j, w)) in inf.iter().enumerate() {
                            v.joints[k] = *j;
                            v.weights[k] = if total > 0.0 { *w / total } else { 0.0 };
                        }
                        sec.vertices.push(v);
                        let vi = sec.vertices.len() as u32 - 1;
                        weld.insert(key, vi);
                        vi
                    }
                };
                emitted.push(vi);
            }
            // Fan triangulation, winding flipped for the axis swap.
            for k in 1..emitted.len().saturating_sub(1) {
                sec.indices.extend_from_slice(&[emitted[0], emitted[k + 1], emitted[k]]);
            }
        }
        for (_, mut sec, _) in sections {
            if sec.indices.is_empty() {
                continue;
            }
            if normals.is_none() {
                compute_normals(&mut sec);
            }
            mesh.sections.push(sec);
        }
        if skinned {
            mesh.skinned = true;
        }
    }
    if mesh.sections.is_empty() {
        return Err(Error::Format("FBX: no mesh with polygons".into()));
    }
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_ops::{Bone, GltfMaterial, Section};

    #[test]
    fn euler_round_trips() {
        for deg in [[0.0, 0.0, 0.0], [30.0, 0.0, 0.0], [10.0, 20.0, 30.0], [-45.0, 60.0, -120.0], [0.0, 89.0, 0.0]] {
            let m = rotation_xyz(deg);
            let back = euler_xyz_deg(&m);
            let m2 = rotation_xyz(back);
            for c in 0..3 {
                for r in 0..3 {
                    assert!((m[c][r] - m2[c][r]).abs() < 1e-5, "{deg:?}: {back:?}");
                }
            }
        }
    }

    #[test]
    fn export_import_round_trips() {
        let g = Geometry {
            positions: vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 100.0, 0.0], [0.0, 100.0, 50.0]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
            sections: vec![Section { first_index: 0, num_triangles: 1, material_name: "mat_a".into(), ..Default::default() }, Section { first_index: 3, num_triangles: 1, material_name: "mat_b".into(), ..Default::default() }],
            influences: vec![[(0, 255), (0, 0), (0, 0), (0, 0)], [(1, 128), (0, 127), (0, 0), (0, 0)], [(1, 255), (0, 0), (0, 0), (0, 0)], [(0, 255), (0, 0), (0, 0), (0, 0)]],
            bone_names: vec!["root".into(), "child".into()],
            bones: vec![
                Bone { name: "root".into(), parent: None, local: crate::mesh_ops::quat_translation_matrix([0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0]) },
                Bone { name: "child".into(), parent: Some(0), local: crate::mesh_ops::quat_translation_matrix([0.0, 0.0, 0.0, 1.0], [10.0, 20.0, 30.0]) },
            ],
            materials: vec![GltfMaterial { name: "mat_a".into(), ..Default::default() }, GltfMaterial { name: "mat_b".into(), ..Default::default() }],
            images: Vec::new(),
        };
        let bytes = write_fbx("test", &g).unwrap();
        let m = parse_fbx(&bytes).unwrap();
        assert_eq!(m.sections.len(), 2);
        assert_eq!(m.sections[0].material, "mat_a");
        assert_eq!(m.sections[1].material, "mat_b");
        assert!(m.skinned);
        assert_eq!(m.joint_names, vec!["root".to_string(), "child".to_string()]);
        let v = &m.sections[0].vertices;
        assert_eq!(v.len(), 3);
        let p1 = v.iter().find(|v| (v.position[0] - 100.0).abs() < 1e-3 && v.position[1].abs() < 1e-3).unwrap();
        assert!((p1.weights[0] - 128.0 / 255.0).abs() < 1e-3 && p1.joints[0] == 1, "{p1:?}");
        assert!((p1.uvs[0][0] - 1.0).abs() < 1e-6 && p1.uvs[0][1].abs() < 1e-6);
        // Engine winding survives the double flip.
        let tri: Vec<[f32; 3]> = m.sections[0].indices.iter().map(|&i| v[i as usize].position).collect();
        for (a, b) in tri.iter().zip(&g.positions[..3]) {
            assert!((a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3 && (a[2] - b[2]).abs() < 1e-3, "{tri:?}");
        }
        let p3 = m.sections[1].vertices.iter().find(|v| (v.position[2] - 50.0).abs() < 1e-3).unwrap();
        assert!((p3.position[1] - 100.0).abs() < 1e-3);
    }
}
