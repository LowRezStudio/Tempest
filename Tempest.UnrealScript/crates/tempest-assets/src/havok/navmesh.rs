//! Typed views of the Havok AI objects a `HavokNavMeshActor` stores
//! (`AHavokNavMeshActor::InitializeNavMesh` in the 8.2 server: the root
//! container's `hkaiNavMesh`, `hkaiDirectedGraphExplicitCost` (the cluster
//! graph), the variant named "Mediator" (`hkaiStaticTreeNavMeshQueryMediator`
//! → `hkcdStaticAabbTree`) and `hkaiTraversalAnnotationLibrary`).
//!
//! Units: Havok space is Unreal space scaled by 1/16, same axes, Z up
//! (`FHavokAiWorld::AddObstacle` multiplies Unreal points by 0.0625).

use tempest_upk::io::{Error, Result};

use super::tagfile::{half_to_f32, Rec, Tagfile, Value, ITEM_ARRAY};
use super::tree::StaticTree;

/// Havok units → Unreal units.
pub const HAVOK_TO_UNREAL: f32 = 16.0;

fn err(s: impl Into<String>) -> Error {
    Error::Format(s.into())
}

/// `hkaiNavMesh::EdgeFlagBits` (8.2 server DWARF).
pub mod edge_flags {
    pub const SILHOUETTE: u8 = 0x1;
    pub const RETRIANGULATED: u8 = 0x2;
    pub const ORIGINAL: u8 = 0x4;
    pub const ORPHANED_USER_EDGE: u8 = 0x8;
    pub const USER: u8 = 0x10;
    pub const BLOCKED: u8 = 0x20;
    pub const EXTERNAL_OPPOSITE: u8 = 0x40;
    pub const GARBAGE: u8 = 0x80;
}

/// `hkaiNavMesh::FaceFlagBits`.
pub mod face_flags {
    pub const HIDDEN: u8 = 0x1;
    pub const CUT: u8 = 0x2;
    pub const STREAMING: u8 = 0x4;
    pub const HAS_DYNAMIC_USER_EDGES: u8 = 0x8;
}

/// `hkaiTraversalType`.
pub const TRAVERSAL_TYPES: [&str; 6] = ["jump", "climb up", "vault", "duck under", "drop down", "balcony vault"];

/// An unset `hkaiPackedKey` / index.
pub const INVALID: u32 = 0xffff_ffff;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Face {
    pub start_edge: i32,
    pub start_user_edge: i32,
    pub num_edges: i16,
    pub num_user_edges: i16,
    pub cluster: i16,
    /// Stored uninitialised (`0xCDCD`); kept as read.
    pub padding: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edge {
    pub a: i32,
    pub b: i32,
    /// Packed key (section index in the top bits, 0 here): `INVALID` on a boundary.
    pub opposite_edge: u32,
    pub opposite_face: u32,
    pub flags: u8,
    pub user_edge_cost: u16,
}

#[derive(Clone, Debug, Default)]
pub struct NavMesh {
    pub faces: Vec<Face>,
    pub edges: Vec<Edge>,
    pub vertices: Vec<[f32; 4]>,
    pub face_data: Vec<i32>,
    pub edge_data: Vec<i32>,
    pub face_data_striding: i32,
    pub edge_data_striding: i32,
    pub flags: u8,
    pub aabb: [[f32; 4]; 2],
    pub erosion_radius: f32,
    pub user_data: u64,
    pub streaming_sets: usize,
    pub has_clearance_seeding: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphEdge {
    pub cost: u16,
    pub flags: u16,
    pub target: u32,
}

/// `hkaiDirectedGraphExplicitCost`: one node per face cluster.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub positions: Vec<[f32; 4]>,
    pub nodes: Vec<(i32, i32)>,
    pub edges: Vec<GraphEdge>,
    pub node_data: Vec<u32>,
    pub edge_data: Vec<u32>,
    pub node_data_striding: i32,
    pub edge_data_striding: i32,
    pub streaming_sets: usize,
}

/// `hkaiUserEdgeUtils::UserEdgePair`: a user edge between two faces (the
/// game's jump links): `x`, `y`, `z` hold the two segments' ends.
#[derive(Clone, Debug, Default)]
pub struct UserEdgePair {
    pub x: [f32; 4],
    pub y: [f32; 4],
    pub z: [f32; 4],
    pub instance_a: u32,
    pub instance_b: u32,
    pub face_a: i32,
    pub face_b: i32,
    pub user_data_a: i32,
    pub user_data_b: i32,
    pub cost_a_to_b: f32,
    pub cost_b_to_a: f32,
    pub direction: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Annotation {
    pub t_equation: [f32; 4],
    pub user_data: u32,
    pub first_partition: u32,
    pub first_vector: u32,
    pub num_partitions: u8,
    pub num_vectors: u8,
    pub traversal_type: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Library {
    pub up: [f32; 4],
    pub user_edge_pairs: Vec<UserEdgePair>,
    pub annotations: Vec<Annotation>,
    pub vectors: Vec<[f32; 4]>,
    pub interval_data: usize,
    pub partition_records: usize,
}

/// The objects of one nav mesh tagfile and where they live.
#[derive(Clone, Debug, Default)]
pub struct NavData {
    pub tf: Tagfile,
    pub mesh_item: u32,
    pub graph_item: u32,
    pub mediator_item: u32,
    pub tree_item: u32,
    pub impl_item: u32,
    pub library_item: u32,
    pub mesh: NavMesh,
    pub graph: Option<Graph>,
    pub tree: Option<StaticTree>,
    pub library: Option<Library>,
}

fn ints<T: Copy>(tf: &Tagfile, item: u32, f: impl Fn(&Value) -> T) -> Result<Vec<T>> {
    if item == 0 {
        return Ok(Vec::new());
    }
    Ok(tf.values(item)?.iter().map(f).collect())
}

fn vec4s(tf: &Tagfile, item: u32) -> Result<Vec<[f32; 4]>> {
    ints(tf, item, |v| v.vec4())
}

fn recs(tf: &Tagfile, item: u32) -> Result<Vec<Rec<'_>>> {
    if item == 0 {
        return Ok(Vec::new());
    }
    Ok(tf.values(item)?.into_iter().filter_map(|v| Rec::new(tf, v)).collect())
}

fn aabb(r: Option<Rec<'_>>) -> [[f32; 4]; 2] {
    match r {
        Some(r) => [r.vec4("min"), r.vec4("max")],
        None => [[0.0; 4]; 2],
    }
}

fn count(tf: &Tagfile, item: u32) -> usize {
    if item == 0 { 0 } else { tf.items.get(item as usize).map(|i| i.count as usize).unwrap_or(0) }
}

impl NavData {
    pub fn read(bytes: &[u8]) -> Result<NavData> {
        NavData::from_tagfile(Tagfile::read(bytes)?)
    }

    pub fn from_tagfile(tf: Tagfile) -> Result<NavData> {
        let mut nd = NavData::default();
        let class = |item: u32| -> String { tf.items.get(item as usize).map(|i| tf.type_name(i.ty)).unwrap_or_default() };
        for (name, _, item) in tf.root_variants()? {
            let c = class(item);
            match c.as_str() {
                "hkaiNavMesh" => nd.mesh_item = item,
                "hkaiDirectedGraphExplicitCost" => nd.graph_item = item,
                "hkaiTraversalAnnotationLibrary" => nd.library_item = item,
                _ if name == "Mediator" => nd.mediator_item = item,
                _ => {}
            }
        }
        if nd.mesh_item == 0 {
            return Err(err("tagfile holds no hkaiNavMesh"));
        }
        nd.mesh = read_mesh(&tf, nd.mesh_item)?;
        if nd.graph_item != 0 {
            nd.graph = Some(read_graph(&tf, nd.graph_item)?);
        }
        if nd.library_item != 0 {
            nd.library = Some(read_library(&tf, nd.library_item)?);
        }
        if nd.mediator_item != 0 {
            let m = Rec::new(&tf, tf.value(nd.mediator_item, 0)?).ok_or_else(|| err("mediator is not a record"))?;
            nd.tree_item = m.item("tree");
            if nd.tree_item != 0 {
                let t = Rec::new(&tf, tf.value(nd.tree_item, 0)?).ok_or_else(|| err("tree is not a record"))?;
                nd.impl_item = t.item("treePtr");
                if nd.impl_item != 0 {
                    let i = Rec::new(&tf, tf.value(nd.impl_item, 0)?).ok_or_else(|| err("tree impl is not a record"))?;
                    let domain = aabb(i.rec("domain"));
                    let nodes_item = i.item("nodes");
                    let nodes = ints(&tf, nodes_item, |v| {
                        let r = Rec::new(&tf, v.clone());
                        let mut n = [0u8; 6];
                        if let Some(r) = r {
                            if let Some(Value::Fixed(x)) = r.get("xyz") {
                                for k in 0..3 {
                                    n[k] = x.get(k).map(|e| e.as_i64() as u8).unwrap_or(0);
                                }
                            }
                            n[3] = r.int("hiData") as u8;
                            let lo = r.int("loData") as u16;
                            n[4..6].copy_from_slice(&lo.to_le_bytes());
                        }
                        n
                    })?;
                    nd.tree = Some(StaticTree { domain, nodes });
                }
            }
        }
        nd.tf = tf;
        Ok(nd)
    }

    /// Faces' vertex loops (vertex indices), in edge order.
    pub fn face_vertices(&self, f: usize) -> Vec<u32> {
        let face = &self.mesh.faces[f];
        (0..face.num_edges.max(0) as usize).filter_map(|k| self.mesh.edges.get(face.start_edge as usize + k)).map(|e| e.a as u32).collect()
    }
}

pub fn read_mesh(tf: &Tagfile, item: u32) -> Result<NavMesh> {
    let r = Rec::new(tf, tf.value(item, 0)?).ok_or_else(|| err("hkaiNavMesh is not a record"))?;
    let faces = recs(tf, r.item("faces"))?
        .iter()
        .map(|f| Face {
            start_edge: f.int("startEdgeIndex") as i32,
            start_user_edge: f.int("startUserEdgeIndex") as i32,
            num_edges: f.int("numEdges") as i16,
            num_user_edges: f.int("numUserEdges") as i16,
            cluster: f.int("clusterIndex") as i16,
            padding: f.int("padding") as u16,
        })
        .collect();
    let edges = recs(tf, r.item("edges"))?
        .iter()
        .map(|e| Edge {
            a: e.int("a") as i32,
            b: e.int("b") as i32,
            opposite_edge: e.int("oppositeEdge") as u32,
            opposite_face: e.int("oppositeFace") as u32,
            flags: e.int("flags") as u8,
            user_edge_cost: match e.get("userEdgeCost") {
                Some(Value::Half(h)) => *h,
                Some(Value::Record(_, f)) => f.first().map(|v| v.as_i64() as u16).unwrap_or(0),
                _ => 0,
            },
        })
        .collect();
    Ok(NavMesh {
        faces,
        edges,
        vertices: vec4s(tf, r.item("vertices"))?,
        face_data: ints(tf, r.item("faceData"), |v| v.as_i64() as i32)?,
        edge_data: ints(tf, r.item("edgeData"), |v| v.as_i64() as i32)?,
        face_data_striding: r.int("faceDataStriding") as i32,
        edge_data_striding: r.int("edgeDataStriding") as i32,
        flags: r.int("flags") as u8,
        aabb: aabb(r.rec("aabb")),
        erosion_radius: r.real("erosionRadius"),
        user_data: r.int("userData") as u64,
        streaming_sets: count(tf, r.item("streamingSets")),
        has_clearance_seeding: r.item("clearanceCacheSeedingDataSet") != 0,
    })
}

pub fn read_graph(tf: &Tagfile, item: u32) -> Result<Graph> {
    let r = Rec::new(tf, tf.value(item, 0)?).ok_or_else(|| err("graph is not a record"))?;
    Ok(Graph {
        positions: vec4s(tf, r.item("positions"))?,
        nodes: recs(tf, r.item("nodes"))?.iter().map(|n| (n.int("startEdgeIndex") as i32, n.int("numEdges") as i32)).collect(),
        edges: recs(tf, r.item("edges"))?
            .iter()
            .map(|e| GraphEdge {
                cost: match e.get("cost") {
                    Some(Value::Half(h)) => *h,
                    _ => 0,
                },
                flags: e.int("flags") as u16,
                target: e.int("target") as u32,
            })
            .collect(),
        node_data: ints(tf, r.item("nodeData"), |v| v.as_i64() as u32)?,
        edge_data: ints(tf, r.item("edgeData"), |v| v.as_i64() as u32)?,
        node_data_striding: r.int("nodeDataStriding") as i32,
        edge_data_striding: r.int("edgeDataStriding") as i32,
        streaming_sets: count(tf, r.item("streamingSets")),
    })
}

pub fn read_library(tf: &Tagfile, item: u32) -> Result<Library> {
    let r = Rec::new(tf, tf.value(item, 0)?).ok_or_else(|| err("annotation library is not a record"))?;
    let ipl = r.rec("intervalPartitionLibrary");
    Ok(Library {
        up: r.vec4("up"),
        user_edge_pairs: recs(tf, r.item("userEdgePairs"))?
            .iter()
            .map(|p| UserEdgePair {
                x: p.vec4("x"),
                y: p.vec4("y"),
                z: p.vec4("z"),
                instance_a: p.int("instanceUidA") as u32,
                instance_b: p.int("instanceUidB") as u32,
                face_a: p.int("faceA") as i32,
                face_b: p.int("faceB") as i32,
                user_data_a: p.int("userDataA") as i32,
                user_data_b: p.int("userDataB") as i32,
                cost_a_to_b: p.real("costAtoB"),
                cost_b_to_a: p.real("costBtoA"),
                direction: p.int("direction") as u8,
            })
            .collect(),
        annotations: recs(tf, r.item("annotations"))?
            .iter()
            .map(|a| Annotation {
                t_equation: a.vec4("tEquation"),
                user_data: a.int("userdata") as u32,
                first_partition: a.int("firstPartitionIndex") as u32,
                first_vector: a.int("firstVectorIndex") as u32,
                num_partitions: a.int("numPartitions") as u8,
                num_vectors: a.int("numVectors") as u8,
                traversal_type: a.int("traversalType") as u8,
            })
            .collect(),
        vectors: vec4s(tf, r.item("vectorLibrary"))?,
        interval_data: ipl.as_ref().map(|i| count(tf, i.item("data"))).unwrap_or(0),
        partition_records: ipl.as_ref().map(|i| count(tf, i.item("partitionRecords"))).unwrap_or(0),
    })
}

/// Structural checks the runtime relies on (`hkaiNavMeshUtils::validate`
/// style): every face's edges in range and chained a→b around the face,
/// opposite edges pointing back, vertex indices in range.
pub fn validate(m: &NavMesh) -> Vec<String> {
    let mut out = Vec::new();
    let nv = m.vertices.len() as i32;
    let ne = m.edges.len() as i32;
    for (fi, f) in m.faces.iter().enumerate() {
        if f.num_edges < 3 || f.start_edge < 0 || f.start_edge + f.num_edges as i32 > ne {
            out.push(format!("face {fi}: edges {}+{} out of range", f.start_edge, f.num_edges));
            continue;
        }
        for k in 0..f.num_edges as i32 {
            let e = &m.edges[(f.start_edge + k) as usize];
            let next = &m.edges[(f.start_edge + (k + 1) % f.num_edges as i32) as usize];
            if e.a < 0 || e.a >= nv || e.b < 0 || e.b >= nv {
                out.push(format!("face {fi} edge {}: vertex out of range", f.start_edge + k));
            }
            if e.b != next.a {
                out.push(format!("face {fi} edge {}: does not chain to the next edge", f.start_edge + k));
            }
            if e.opposite_edge != INVALID && e.flags & edge_flags::EXTERNAL_OPPOSITE == 0 {
                let oe = e.opposite_edge as usize;
                let of = e.opposite_face as usize;
                match (m.edges.get(oe), m.faces.get(of)) {
                    // Opposites need not share vertices: 6 pairs of the
                    // shipped meshes (FB_Dragon_Ruins…) only overlap in part.
                    (Some(o), Some(off)) => {
                        if o.opposite_edge as i32 != f.start_edge + k {
                            out.push(format!("edge {}: opposite {oe} does not point back", f.start_edge + k));
                        }
                        if (oe as i32) < off.start_edge || oe as i32 >= off.start_edge + off.num_edges as i32 {
                            out.push(format!("edge {}: opposite edge {oe} is not in opposite face {of}", f.start_edge + k));
                        }
                    }
                    _ => out.push(format!("edge {}: opposite {oe}/{of} out of range", f.start_edge + k)),
                }
            }
        }
    }
    out
}

/// Text summary (`upk havok dump`).
pub fn describe(nd: &NavData, full: bool) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let m = &nd.mesh;
    let boundary = m.edges.iter().filter(|e| e.opposite_edge == INVALID).count();
    let mut flags = std::collections::BTreeMap::new();
    for e in &m.edges {
        *flags.entry(e.flags).or_insert(0usize) += 1;
    }
    let _ = writeln!(s, "hkaiNavMesh (item {}): {} faces, {} edges ({boundary} boundary), {} vertices", nd.mesh_item, m.faces.len(), m.edges.len(), m.vertices.len());
    let _ = writeln!(s, "  aabb {:?} .. {:?}  erosion radius {}  flags {:#x}  user data {:#x}", &m.aabb[0][..3], &m.aabb[1][..3], m.erosion_radius, m.flags, m.user_data);
    let _ = writeln!(s, "  face data {} (striding {}), edge data {} (striding {}), streaming sets {}, clearance seeding {}", m.face_data.len(), m.face_data_striding, m.edge_data.len(), m.edge_data_striding, m.streaming_sets, m.has_clearance_seeding);
    let _ = writeln!(s, "  edge flags: {}", flags.iter().map(|(f, n)| format!("{f:#x}×{n}")).collect::<Vec<_>>().join(" "));
    let user = m.faces.iter().filter(|f| f.num_user_edges > 0).count();
    let clusters = m.faces.iter().map(|f| f.cluster).max().map(|c| c as i32 + 1).unwrap_or(0);
    let _ = writeln!(s, "  clusters {clusters}, faces with user edges {user}");
    let problems = validate(m);
    let _ = writeln!(s, "  validation: {}", if problems.is_empty() { "ok".to_string() } else { format!("{} problems: {}", problems.len(), problems.join("; ")) });
    if let Some(g) = &nd.graph {
        let _ = writeln!(s, "hkaiDirectedGraphExplicitCost (item {}): {} nodes, {} edges, node data {} (striding {}), edge data {} (striding {}), streaming sets {}", nd.graph_item, g.nodes.len(), g.edges.len(), g.node_data.len(), g.node_data_striding, g.edge_data.len(), g.edge_data_striding, g.streaming_sets);
    }
    if let Some(t) = &nd.tree {
        let _ = writeln!(s, "hkcdStaticAabbTree (item {}): {} nodes, {} leaves, domain {:?} .. {:?}", nd.tree_item, t.nodes.len(), t.leaves().len(), t.domain[0], t.domain[1]);
    }
    if let Some(l) = &nd.library {
        let mut types = std::collections::BTreeMap::new();
        for a in &l.annotations {
            *types.entry(a.traversal_type).or_insert(0usize) += 1;
        }
        let _ = writeln!(
            s,
            "hkaiTraversalAnnotationLibrary (item {}): up {:?}, {} user edge pairs, {} annotations ({}), {} vectors, intervals {} / {} records",
            nd.library_item,
            &l.up[..3],
            l.user_edge_pairs.len(),
            l.annotations.len(),
            types.iter().map(|(t, n)| format!("{}×{n}", TRAVERSAL_TYPES.get(*t as usize).copied().unwrap_or("custom"))).collect::<Vec<_>>().join(", "),
            l.vectors.len(),
            l.interval_data,
            l.partition_records
        );
    }
    if full {
        for (i, f) in m.faces.iter().enumerate() {
            let _ = writeln!(s, "face {i}: edges {}+{} user {}+{} cluster {} data {:?} verts {:?}", f.start_edge, f.num_edges, f.start_user_edge, f.num_user_edges, f.cluster, m.face_data.get(i), nd.face_vertices(i));
        }
        if let Some(g) = &nd.graph {
            for (i, (s0, n)) in g.nodes.iter().enumerate() {
                let es: Vec<String> = (0..*n).filter_map(|k| g.edges.get((*s0 + k) as usize)).map(|e| format!("{}@{:.2}", e.target, half_to_f32(e.cost))).collect();
                let _ = writeln!(s, "cluster {i}: {:?} → {}", &g.positions[i][..3], es.join(" "));
            }
        }
        if let Some(l) = &nd.library {
            for (i, p) in l.user_edge_pairs.iter().enumerate() {
                let _ = writeln!(s, "user edge {i}: faces {}→{} x {:?} y {:?} z {:?} cost {}/{} dir {} data {:#x}/{:#x}", p.face_a, p.face_b, &p.x[..3], &p.y[..3], &p.z[..3], p.cost_a_to_b, p.cost_b_to_a, p.direction, p.user_data_a, p.user_data_b);
            }
            for (i, a) in l.annotations.iter().enumerate() {
                let _ = writeln!(s, "annotation {i}: {:?} type {} user {:#x} partitions {}+{} vectors {}+{}", a.t_equation, a.traversal_type, a.user_data, a.first_partition, a.num_partitions, a.first_vector, a.num_vectors);
            }
        }
    }
    s
}

/// Item index of an array member of the record in `owner`, after making
/// sure an item exists for it (element type from the array type).
pub fn array_item(tf: &mut Tagfile, owner: u32, member: &str) -> Result<u32> {
    let v = tf.value(owner, 0)?;
    let ty = tf.items[owner as usize].ty;
    let mut r = Rec::new(tf, v).ok_or_else(|| err("not a record"))?;
    let cur = r.item(member);
    if cur != 0 {
        return Ok(cur);
    }
    let m = tf.members(ty).into_iter().find(|m| tf.member_name(m) == member).ok_or_else(|| err(format!("no member {member}")))?.clone();
    let et = tf.sub(m.ty);
    r.set(member, Value::Array(0));
    let item = tf.items.len() as u32;
    r.set(member, Value::Array(item));
    let v = r.into_value();
    tf.add_item(ITEM_ARRAY, et, 0, Vec::new());
    set_record(tf, owner, &v)?;
    Ok(item)
}

/// Re-encode the record held by item `owner`.
pub fn set_record(tf: &mut Tagfile, owner: u32, v: &Value) -> Result<()> {
    let ty = tf.items[owner as usize].ty;
    let mut b = std::mem::take(&mut tf.items[owner as usize].bytes);
    let r = tf.encode(ty, v, &mut b);
    tf.items[owner as usize].bytes = b;
    r
}

/// Replace every element of an array item; elements that existed keep the
/// bytes no member covers.
pub fn set_elements(tf: &mut Tagfile, item: u32, values: &[Value]) -> Result<()> {
    let ty = tf.items[item as usize].ty;
    let sz = tf.size(ty) as usize;
    let mut b = std::mem::take(&mut tf.items[item as usize].bytes);
    b.resize(values.len() * sz, 0);
    let mut res = Ok(());
    for (i, v) in values.iter().enumerate() {
        if let Err(e) = tf.encode(ty, v, &mut b[i * sz..(i + 1) * sz]) {
            res = Err(e);
            break;
        }
    }
    let it = &mut tf.items[item as usize];
    it.bytes = b;
    it.count = values.len() as u32;
    res
}
