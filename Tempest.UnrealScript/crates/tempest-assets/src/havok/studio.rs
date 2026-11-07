//! The Studio's nav mesh methods (`map.navmesh`, `map.navmeshEdit` in
//! `serve.rs`): the level's `HavokNavMeshActor`s as view-space buffers,
//! and edits written back into the actor's bulk data as an undoable,
//! unsaved edit of the level.

use serde_json::{json, Value as J};
use tempest_upk::io::{Error, Result};
use tempest_upk::PackageIndex;

use super::navmesh::{NavData, TRAVERSAL_TYPES};
use super::{actor_tagfile, edit, set_actor_tagfile, view};
use crate::edit::Editor;
use crate::serve::BinSink;

fn err(s: impl Into<String>) -> Error {
    Error::Format(s.into())
}

/// Every `HavokNavMeshActor` export of the package.
pub fn actors(ed: &Editor) -> Vec<usize> {
    (0..ed.pkg.exports.len()).filter(|&i| ed.pkg.class_name_of(PackageIndex::export(i)).eq_ignore_ascii_case("HavokNavMeshActor")).collect()
}

pub fn read(ed: &mut Editor, i: usize) -> Result<Option<NavData>> {
    let bytes = ed.with_object(i, |_, o| actor_tagfile(o))??;
    bytes.map(|b| NavData::read(&b)).transpose()
}

/// One actor's buffers (binary payloads) and counts.
pub fn actor_json(ed: &Editor, i: usize, nd: &NavData, bins: &mut BinSink) -> J {
    let b = view::buffers(nd);
    let clusters = nd.mesh.faces.iter().map(|f| f.cluster as i64).max().map(|c| c + 1).unwrap_or(0);
    let links: Vec<J> = nd
        .library
        .as_ref()
        .map(|l| {
            l.user_edge_pairs
                .iter()
                .enumerate()
                .map(|(k, p)| {
                    let a = l.annotations.get(k);
                    json!({
                        "faceA": p.face_a,
                        "faceB": p.face_b,
                        "costAtoB": p.cost_a_to_b,
                        "costBtoA": p.cost_b_to_a,
                        "direction": p.direction,
                        "type": a.map(|a| TRAVERSAL_TYPES.get(a.traversal_type as usize).copied().unwrap_or("custom")),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "index": i,
        "path": ed.pkg.full_name(PackageIndex::export(i)),
        "faces": nd.mesh.faces.len(),
        "edges": nd.mesh.edges.len(),
        "vertices": nd.mesh.vertices.len(),
        "clusters": clusters,
        "regions": b.regions,
        "boundary": b.line_kind.iter().filter(|k| **k == 1).count(),
        "erosionRadius": nd.mesh.erosion_radius,
        "problems": super::navmesh::validate(&nd.mesh).len(),
        "linkInfo": links,
        "positions": bins.add(view::f32_bytes(&b.positions)),
        "triangles": bins.add(view::u32_bytes(&b.triangles)),
        "triFace": bins.add(view::u32_bytes(&b.tri_face)),
        "faceCluster": bins.add(view::i32_bytes(&b.face_cluster)),
        "faceRegion": bins.add(view::i32_bytes(&b.face_region)),
        "lines": bins.add(view::u32_bytes(&b.lines)),
        "lineKind": bins.add(b.line_kind.clone()),
        "links": bins.add(view::f32_bytes(&b.links)),
    })
}

/// `map.navmesh {path, index?}`.
pub fn navmesh(ed: &mut Editor, only: Option<usize>, bins: &mut BinSink) -> Result<J> {
    let mut out = Vec::new();
    for i in actors(ed) {
        if only.is_some_and(|o| o != i) {
            continue;
        }
        if let Some(nd) = read(ed, i)? {
            out.push(actor_json(ed, i, &nd, bins));
        }
    }
    Ok(json!({ "actors": out, "dirty": ed.is_dirty() }))
}

/// `map.navmeshEdit {path, index, op, faces?, moves?}`: `deleteFaces`
/// (`faces`), `deleteRegion` (every face of the regions of `faces`),
/// `moveVertices` (`moves`: `[vertex, x, y, z]` in view space). One undo step.
pub fn navmesh_edit(ed: &mut Editor, i: usize, p: &J, bins: &mut BinSink) -> Result<J> {
    let mut nd = read(ed, i)?.ok_or_else(|| err("this actor holds no nav mesh"))?;
    let op = p.get("op").and_then(|v| v.as_str()).unwrap_or("");
    let faces: Vec<usize> = p.get("faces").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64()).map(|x| x as usize).collect()).unwrap_or_default();
    let label = match op {
        "deleteFaces" => {
            let n = edit::delete_faces(&mut nd, &faces)?;
            format!("delete {n} nav mesh faces")
        }
        "deleteRegion" => {
            let (region, _) = view::regions(&nd);
            let picked: std::collections::HashSet<i32> = faces.iter().filter_map(|&f| region.get(f).copied()).collect();
            let all: Vec<usize> = (0..region.len()).filter(|&f| picked.contains(&region[f])).collect();
            let n = edit::delete_faces(&mut nd, &all)?;
            format!("cut a nav mesh region ({n} faces)")
        }
        "moveVertices" => {
            let mut moves = Vec::new();
            for m in p.get("moves").and_then(|v| v.as_array()).into_iter().flatten() {
                let a: Vec<f64> = m.as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default();
                if a.len() == 4 {
                    moves.push((a[0] as usize, view::from_view([a[1] as f32, a[2] as f32, a[3] as f32])));
                }
            }
            edit::move_vertices(&mut nd, &moves)?;
            format!("move {} nav mesh vertices", moves.len())
        }
        _ => return Err(err(format!("unknown nav mesh edit {op:?}"))),
    };
    let problems = super::navmesh::validate(&nd.mesh);
    if !problems.is_empty() {
        return Err(err(format!("the edit would leave the nav mesh invalid: {}", problems[0])));
    }
    let bytes = edit::write_back(&mut nd)?;
    // Read back what was written: the stored form is what the next call sees.
    let nd = NavData::read(&bytes)?;
    let mut obj = ed.object(i)?.clone();
    set_actor_tagfile(&mut obj, &bytes)?;
    ed.begin_undo(label);
    ed.replace_object(i, obj)?;
    let mut v = actor_json(ed, i, &nd, bins);
    v["dirty"] = J::from(true);
    Ok(v)
}
