//! Editing a nav mesh: delete faces (a selection or a whole region), move
//! vertices, then write the typed objects back into the tagfile and
//! rebuild what depends on the faces (the mediator's AABB tree, the
//! cluster graph's links).
//!
//! Kept valid as `hkaiNavMesh` requires: faces own contiguous edge ranges
//! (`startEdgeIndex`/`numEdges`, user edges `startUserEdgeIndex`/
//! `numUserEdges`, stored after every face's own edges), opposite edges and
//! faces renumbered (an edge whose opposite face is gone becomes a boundary;
//! a user edge whose other side is gone is dropped), per-face and per-edge
//! data (`faceData`/`edgeData` with their striding) follow their owners.
//! The annotation library's `userEdgePairs` name faces: pairs touching a
//! deleted face are dropped, the others renumbered; `annotations` are left
//! as they are (user edges reach them through their user data). The
//! clearance cache seeding (precomputed per face and edge) no longer
//! matches, so it is dropped; Havok computes clearances lazily without it.

use std::collections::HashSet;

use tempest_upk::io::{Error, Result};

use super::navmesh::{array_item, set_elements, set_record, NavData, INVALID};
use super::tagfile::{f32_to_half, kind, Rec, Tagfile, Value};
use super::build;

fn err(s: impl Into<String>) -> Error {
    Error::Format(s.into())
}

/// Faces to delete → the mesh without them.
pub fn delete_faces(nd: &mut NavData, faces: &[usize]) -> Result<usize> {
    let m = &mut nd.mesh;
    let gone: HashSet<usize> = faces.iter().copied().filter(|&f| f < m.faces.len()).collect();
    if gone.is_empty() {
        return Ok(0);
    }
    if gone.len() == m.faces.len() {
        return Err(err("a nav mesh needs at least one face"));
    }
    // Face map.
    let mut face_map = vec![INVALID; m.faces.len()];
    let mut n = 0u32;
    for (i, slot) in face_map.iter_mut().enumerate() {
        if !gone.contains(&i) {
            *slot = n;
            n += 1;
        }
    }
    // Edges to drop: the deleted faces' own and user edges, and user edges
    // leading to a deleted face.
    let mut drop = vec![false; m.edges.len()];
    for (fi, f) in m.faces.iter().enumerate() {
        let user = (f.start_user_edge.max(0) as usize)..(f.start_user_edge.max(0) as usize + f.num_user_edges.max(0) as usize);
        let own = (f.start_edge.max(0) as usize)..(f.start_edge.max(0) as usize + f.num_edges.max(0) as usize);
        if gone.contains(&fi) {
            for e in own.chain(if f.num_user_edges > 0 { user } else { 0..0 }) {
                if let Some(d) = drop.get_mut(e) {
                    *d = true;
                }
            }
        } else if f.num_user_edges > 0 {
            for e in user {
                if let Some(edge) = m.edges.get(e) {
                    if edge.opposite_face != INVALID && gone.contains(&(edge.opposite_face as usize)) {
                        drop[e] = true;
                    }
                }
            }
        }
    }
    let mut edge_map = vec![INVALID; m.edges.len()];
    let mut k = 0u32;
    for (i, slot) in edge_map.iter_mut().enumerate() {
        if !drop[i] {
            *slot = k;
            k += 1;
        }
    }
    let first_kept = |start: i32, count: i16| -> (i32, i16) {
        let mut first = -1;
        let mut c = 0i16;
        for e in start.max(0) as usize..(start.max(0) as usize + count.max(0) as usize) {
            if !drop[e] {
                if first < 0 {
                    first = edge_map[e] as i32;
                }
                c += 1;
            }
        }
        (first, c)
    };
    let mut new_faces = Vec::with_capacity(n as usize);
    let mut new_face_data = Vec::new();
    let fs = m.face_data_striding.max(0) as usize;
    for (fi, f) in m.faces.iter().enumerate() {
        if gone.contains(&fi) {
            continue;
        }
        let mut f = f.clone();
        f.start_edge = edge_map[f.start_edge as usize] as i32;
        if f.num_user_edges > 0 {
            let (s, c) = first_kept(f.start_user_edge, f.num_user_edges);
            f.start_user_edge = s;
            f.num_user_edges = c;
        }
        new_faces.push(f);
        if fs > 0 {
            new_face_data.extend_from_slice(&m.face_data[fi * fs..(fi + 1) * fs]);
        }
    }
    let es = m.edge_data_striding.max(0) as usize;
    let mut new_edges = Vec::with_capacity(k as usize);
    let mut new_edge_data = Vec::new();
    for (ei, e) in m.edges.iter().enumerate() {
        if drop[ei] {
            continue;
        }
        let mut e = e.clone();
        let of = e.opposite_face;
        if of != INVALID && e.flags & super::navmesh::edge_flags::EXTERNAL_OPPOSITE == 0 {
            if gone.contains(&(of as usize)) || edge_map.get(e.opposite_edge as usize).copied().unwrap_or(INVALID) == INVALID {
                e.opposite_edge = INVALID;
                e.opposite_face = INVALID;
            } else {
                e.opposite_edge = edge_map[e.opposite_edge as usize];
                e.opposite_face = face_map[of as usize];
            }
        }
        new_edges.push(e);
        if es > 0 {
            new_edge_data.extend_from_slice(&m.edge_data[ei * es..(ei + 1) * es]);
        }
    }
    m.faces = new_faces;
    m.edges = new_edges;
    m.face_data = new_face_data;
    m.edge_data = new_edge_data;
    m.has_clearance_seeding = false;
    if let Some(l) = &mut nd.library {
        l.user_edge_pairs.retain(|p| !gone.contains(&(p.face_a as usize)) && !gone.contains(&(p.face_b as usize)));
        for p in &mut l.user_edge_pairs {
            p.face_a = face_map[p.face_a as usize] as i32;
            p.face_b = face_map[p.face_b as usize] as i32;
        }
    }
    Ok(gone.len())
}

/// Move vertices to new Havok positions (the w component is kept).
pub fn move_vertices(nd: &mut NavData, moves: &[(usize, [f32; 3])]) -> Result<()> {
    for &(i, p) in moves {
        let v = nd.mesh.vertices.get_mut(i).ok_or_else(|| err(format!("no vertex {i}")))?;
        v[0] = p[0];
        v[1] = p[1];
        v[2] = p[2];
    }
    // Keep the mesh bounds around every vertex.
    let a = &mut nd.mesh.aabb;
    for v in &nd.mesh.vertices {
        for k in 0..3 {
            a[0][k] = a[0][k].min(v[k]);
            a[1][k] = a[1][k].max(v[k]);
        }
    }
    Ok(())
}

/// A value of an integer or float member type from a number.
fn scalar(tf: &Tagfile, ty: u32, v: f64) -> Value {
    let fmt = tf.format(ty);
    match fmt & 0xf {
        kind::FLOAT => match tf.size(ty) {
            2 => Value::Half(f32_to_half(v as f32)),
            4 => Value::F32(v as f32),
            _ => Value::F64(v),
        },
        kind::INT if fmt & kind::SIGNED != 0 => Value::Int(v as i64),
        kind::BOOL => Value::Bool(v as u64),
        kind::RECORD => {
            // One-member wrappers (hkHalf16 as a record, hkBool).
            let ms = tf.members(ty);
            Value::Record(ty, ms.iter().map(|m| scalar(tf, m.ty, v)).collect())
        }
        _ => Value::UInt(v as i64 as u64),
    }
}

/// Element type of an array member.
fn elem_type(tf: &Tagfile, owner_ty: u32, member: &str) -> Result<u32> {
    let m = tf.members(owner_ty).into_iter().find(|m| tf.member_name(m) == member).ok_or_else(|| err(format!("no member {member}")))?;
    Ok(tf.sub(m.ty))
}

/// Build one record element: the existing element `i` of `item` when there
/// is one (so bytes no member covers stay), else zeros; then `set` fields.
fn element(tf: &Tagfile, ty: u32, existing: Option<&[u8]>, fields: &[(&str, f64)], vecs: &[(&str, [f32; 4])]) -> Result<Value> {
    let size = tf.size(ty) as usize;
    let zero = vec![0u8; size];
    let v = tf.decode(ty, existing.unwrap_or(&zero))?;
    let mut r = Rec::new(tf, v).ok_or_else(|| err("not a record"))?;
    let types: Vec<(String, u32)> = tf.members(ty).into_iter().map(|m| (tf.member_name(m).to_string(), m.ty)).collect();
    for (name, x) in fields {
        let mt = types.iter().rev().find(|(n, _)| n == name).map(|(_, t)| *t).ok_or_else(|| err(format!("no member {name}")))?;
        r.set(name, scalar(tf, mt, *x));
    }
    for (name, x) in vecs {
        r.set(name, Value::Fixed(x.iter().map(|f| Value::F32(*f)).collect()));
    }
    Ok(r.into_value())
}

fn existing(tf: &Tagfile, item: u32, i: usize) -> Option<&[u8]> {
    if item == 0 {
        return None;
    }
    let it = &tf.items[item as usize];
    let sz = tf.size(it.ty) as usize;
    it.bytes.get(i * sz..(i + 1) * sz)
}

fn put_array(tf: &mut Tagfile, owner: u32, member: &str, values: Vec<Value>) -> Result<()> {
    let item = array_item(tf, owner, member)?;
    set_elements(tf, item, &values)
}

/// Write the typed mesh (and library pairs) back into `nd.tf`, rebuild the
/// AABB tree, then serialise.
pub fn write_back(nd: &mut NavData) -> Result<Vec<u8>> {
    let mesh_ty = nd.tf.items[nd.mesh_item as usize].ty;
    let tf = &nd.tf;
    let face_ty = elem_type(tf, mesh_ty, "faces")?;
    let edge_ty = elem_type(tf, mesh_ty, "edges")?;
    let vert_ty = elem_type(tf, mesh_ty, "vertices")?;
    let int_ty = elem_type(tf, mesh_ty, "faceData")?;
    let mesh_rec = Rec::new(tf, tf.value(nd.mesh_item, 0)?).ok_or_else(|| err("bad mesh"))?;
    let (faces_item, edges_item) = (mesh_rec.item("faces"), mesh_rec.item("edges"));
    let m = &nd.mesh;
    let faces: Vec<Value> = m
        .faces
        .iter()
        .enumerate()
        .map(|(i, f)| {
            element(
                tf,
                face_ty,
                existing(tf, faces_item, i),
                &[("startEdgeIndex", f.start_edge as f64), ("startUserEdgeIndex", f.start_user_edge as f64), ("numEdges", f.num_edges as f64), ("numUserEdges", f.num_user_edges as f64), ("clusterIndex", f.cluster as f64), ("padding", f.padding as f64)],
                &[],
            )
        })
        .collect::<Result<_>>()?;
    let edges: Vec<Value> = m
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let mut v = element(tf, edge_ty, existing(tf, edges_item, i), &[("a", e.a as f64), ("b", e.b as f64), ("oppositeEdge", e.opposite_edge as f64), ("oppositeFace", e.opposite_face as f64), ("flags", e.flags as f64)], &[])?;
            // The cost is an hkHalf16 (format: 16-bit float): keep its bits.
            if let Some(mut r) = Rec::new(tf, v.clone()) {
                r.set("userEdgeCost", Value::Half(e.user_edge_cost));
                v = r.into_value();
            }
            Ok(v)
        })
        .collect::<Result<_>>()?;
    let verts: Vec<Value> = m.vertices.iter().map(|v| Value::Fixed(v.iter().map(|f| Value::F32(*f)).collect())).collect();
    let fd: Vec<Value> = m.face_data.iter().map(|x| scalar(tf, int_ty, *x as f64)).collect();
    let ed: Vec<Value> = m.edge_data.iter().map(|x| scalar(tf, int_ty, *x as f64)).collect();
    let _ = vert_ty;
    let aabb = m.aabb;
    let seeding = m.has_clearance_seeding;
    let mesh_item = nd.mesh_item;
    put_array(&mut nd.tf, mesh_item, "faces", faces)?;
    put_array(&mut nd.tf, mesh_item, "edges", edges)?;
    put_array(&mut nd.tf, mesh_item, "vertices", verts)?;
    if !fd.is_empty() {
        put_array(&mut nd.tf, mesh_item, "faceData", fd)?;
    }
    if !ed.is_empty() {
        put_array(&mut nd.tf, mesh_item, "edgeData", ed)?;
    }
    {
        let tf = &nd.tf;
        let mut r = Rec::new(tf, tf.value(mesh_item, 0)?).ok_or_else(|| err("bad mesh"))?;
        if let Some(mut a) = r.rec("aabb") {
            a.set("min", Value::Fixed(aabb[0].iter().map(|f| Value::F32(*f)).collect()));
            a.set("max", Value::Fixed(aabb[1].iter().map(|f| Value::F32(*f)).collect()));
            r.set("aabb", a.into_value());
        }
        if !seeding {
            // An hkRefPtr's format is a pointer: the member decodes as one.
            match r.get("clearanceCacheSeedingDataSet").cloned() {
                Some(Value::Record(t, f)) => {
                    r.set("clearanceCacheSeedingDataSet", Value::Record(t, f.iter().map(|_| Value::Ptr(0)).collect()));
                }
                Some(Value::Ptr(_)) => {
                    r.set("clearanceCacheSeedingDataSet", Value::Ptr(0));
                }
                _ => {}
            }
        }
        let v = r.into_value();
        set_record(&mut nd.tf, mesh_item, &v)?;
    }
    // Library user edge pairs.
    if let (Some(l), true) = (&nd.library, nd.library_item != 0) {
        let tf = &nd.tf;
        let lib_ty = tf.items[nd.library_item as usize].ty;
        let pair_ty = elem_type(tf, lib_ty, "userEdgePairs")?;
        let pairs_item = Rec::new(tf, tf.value(nd.library_item, 0)?).map(|r| r.item("userEdgePairs")).unwrap_or(0);
        if pairs_item != 0 {
            // Pairs are only dropped or renumbered: rebuild each from its
            // stored element (found by its old position) with the new faces.
            let old = super::navmesh::read_library(tf, nd.library_item)?;
            let mut values = Vec::new();
            for p in &l.user_edge_pairs {
                let j = old.user_edge_pairs.iter().position(|o| o.x == p.x && o.y == p.y && o.z == p.z && o.user_data_a == p.user_data_a).unwrap_or(0);
                values.push(element(tf, pair_ty, existing(tf, pairs_item, j), &[("faceA", p.face_a as f64), ("faceB", p.face_b as f64)], &[])?);
            }
            set_elements(&mut nd.tf, pairs_item, &values)?;
        }
    }
    rebuild_tree(nd)?;
    Ok(nd.tf.write())
}

/// Rebuild the mediator's static AABB tree from the faces.
pub fn rebuild_tree(nd: &mut NavData) -> Result<()> {
    if nd.impl_item == 0 {
        return Ok(());
    }
    let t = build::build_for_mesh(&nd.mesh);
    let tf = &nd.tf;
    let impl_ty = tf.items[nd.impl_item as usize].ty;
    let node_ty = elem_type(tf, impl_ty, "nodes")?;
    let mut r = Rec::new(tf, tf.value(nd.impl_item, 0)?).ok_or_else(|| err("bad tree"))?;
    if let Some(mut d) = r.rec("domain") {
        d.set("min", Value::Fixed(t.domain[0].iter().map(|f| Value::F32(*f)).collect()));
        d.set("max", Value::Fixed(t.domain[1].iter().map(|f| Value::F32(*f)).collect()));
        r.set("domain", d.into_value());
    }
    let nodes_item = r.item("nodes");
    let v = r.into_value();
    let values: Vec<Value> = t
        .nodes
        .iter()
        .map(|n| {
            let zero = vec![0u8; tf.size(node_ty) as usize];
            let mut r = Rec::new(tf, tf.decode(node_ty, &zero)?).ok_or_else(|| err("bad node"))?;
            r.set("xyz", Value::Fixed(n[0..3].iter().map(|b| Value::UInt(*b as u64)).collect()));
            r.set("hiData", Value::UInt(n[3] as u64));
            r.set("loData", Value::UInt(u16::from_le_bytes([n[4], n[5]]) as u64));
            Ok(r.into_value())
        })
        .collect::<Result<_>>()?;
    let impl_item = nd.impl_item;
    set_record(&mut nd.tf, impl_item, &v)?;
    if nodes_item != 0 {
        set_elements(&mut nd.tf, nodes_item, &values)?;
    }
    nd.tree = Some(t);
    Ok(())
}
