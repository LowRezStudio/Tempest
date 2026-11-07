//! `upk havok ...`: the Havok nav mesh tagfiles of `HavokNavMeshActor`s.

use std::path::{Path, PathBuf};

use tempest_assets::havok::navmesh::{self, NavData};
use tempest_assets::havok::tagfile::Tagfile;
use tempest_upk::{PackageFile, PackageIndex};

type Res = Result<(), Box<dyn std::error::Error>>;

/// `(export index, path, tagfile)` of every nav mesh actor with data.
fn actors(path: &Path) -> Result<Vec<(usize, String, Vec<u8>)>, Box<dyn std::error::Error>> {
    let pf = PackageFile::open(path)?;
    let pkg = pf.tables();
    let mut out = Vec::new();
    for i in 0..pkg.exports.len() {
        if !pkg.class_name_of(PackageIndex::export(i)).eq_ignore_ascii_case("HavokNavMeshActor") {
            continue;
        }
        let data = pf.export_data(i)?;
        let Some(start) = Tagfile::find(&data) else { continue };
        let len = Tagfile::stored_len(&data[start..]).unwrap_or(0);
        out.push((i, pkg.full_name(PackageIndex::export(i)), data[start..start + len].to_vec()));
    }
    Ok(out)
}

fn packages(args: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for a in args.iter().filter(|a| !a.starts_with('-')) {
        let p = PathBuf::from(a);
        if p.is_dir() {
            let mut v: Vec<PathBuf> = std::fs::read_dir(&p).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("upk"))).collect();
            v.sort();
            out.extend(v);
        } else {
            out.push(p);
        }
    }
    out
}

pub fn cmd(args: &[String]) -> Res {
    match args[0].as_str() {
        "verify" => verify(&args[1..]),
        "dump" | "types" | "extract" => {
            let path = Path::new(&args[1]);
            let all = actors(path)?;
            let pick = args.get(2).filter(|a| !a.starts_with('-')).and_then(|a| a.parse::<usize>().ok());
            for (i, name, bytes) in all.iter().filter(|(i, _, _)| pick.is_none_or(|p| p == *i)) {
                match args[0].as_str() {
                    "extract" => {
                        let out = args.get(3).ok_or("usage: upk havok extract <pkg> <idx> <out.hkt>")?;
                        std::fs::write(out, bytes)?;
                        eprintln!("wrote {} bytes ({name}) to {out}", bytes.len());
                    }
                    "types" => {
                        let tf = Tagfile::read(bytes)?;
                        println!("{i} {name}");
                        print!("{}", types_text(&tf));
                    }
                    _ => {
                        let nd = NavData::read(bytes)?;
                        println!("{i} {name}: {} bytes, SDK {}", bytes.len(), String::from_utf8_lossy(&nd.tf.sdk));
                        print!("{}", navmesh::describe(&nd, args.iter().any(|a| a == "--full")));
                    }
                }
            }
            Ok(())
        }
        _ => Err("usage: upk havok verify|dump|types|extract ...".into()),
    }
}

fn types_text(tf: &Tagfile) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for ty in 1..tf.types.len() as u32 {
        let fmt = tf.format(ty);
        let _ = writeln!(s, "{ty:4} {}  format {fmt:#x} size {} align {}", tf.type_name(ty), tf.size(ty), tf.align(ty));
        if let Some(b) = &tf.types[ty as usize].body {
            for m in &b.members {
                let _ = writeln!(s, "       +{:<3} {} : {} (flags {:#x})", m.offset, tf.member_name(m), tf.type_name(m.ty), m.flags);
            }
        }
    }
    for (i, it) in tf.items.iter().enumerate().skip(1) {
        let _ = writeln!(s, "item {i}: {} ×{} flags {:#x} at {:#x}", tf.type_name(it.ty), it.count, it.flags, it.offset);
    }
    s
}

fn verify(args: &[String]) -> Res {
    let verbose = args.iter().any(|a| a == "-v");
    let (mut n, mut bad, mut values, mut uncovered, mut problems) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut faces = 0usize;
    let (mut trees, mut trees_same) = (0usize, 0usize);
    for p in packages(args) {
        let list = match actors(&p) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("{}: {e}", p.display());
                bad += 1;
                continue;
            }
        };
        for (i, name, bytes) in list {
            n += 1;
            let r = (|| -> Result<String, Box<dyn std::error::Error>> {
                let tf = Tagfile::read(&bytes)?;
                let out = tf.write();
                if out != bytes {
                    let at = out.iter().zip(&bytes).position(|(a, b)| a != b).unwrap_or(out.len().min(bytes.len()));
                    return Err(format!("rewrite differs at {at:#x} ({} vs {} bytes)", out.len(), bytes.len()).into());
                }
                let rep = tf.check_values()?;
                values += rep.values;
                uncovered += rep.uncovered_nonzero;
                let nd = NavData::from_tagfile(tf)?;
                let v = navmesh::validate(&nd.mesh);
                problems += v.len();
                faces += nd.mesh.faces.len();
                if let Some(t) = &nd.tree {
                    let mut seen = vec![false; nd.mesh.faces.len()];
                    for (_, f) in t.leaves() {
                        if let Some(s) = seen.get_mut(f as usize) {
                            *s = true;
                        }
                    }
                    if seen.iter().any(|s| !s) {
                        return Err("the AABB tree misses faces".into());
                    }
                    // Havok's construction, from the faces alone.
                    let rebuilt = tempest_assets::havok::build::build_for_mesh(&nd.mesh);
                    trees += 1;
                    if rebuilt == *t {
                        trees_same += 1;
                    } else if verbose {
                        let first = rebuilt.nodes.iter().zip(&t.nodes).position(|(a, b)| a != b);
                        println!("tree differs: {} {i} ({} faces): domain same {}, first node {:?} of {}", p.display(), nd.mesh.faces.len(), rebuilt.domain == t.domain, first, t.nodes.len());
                        if std::env::var_os("HAVOK_TREE_DEBUG").is_some() {
                            for k in first.unwrap_or(0).saturating_sub(8)..(first.unwrap_or(0) + 6).min(t.nodes.len()) {
                                println!("  {k:3} stored {:02x?} ours {:02x?}", t.nodes[k], rebuilt.nodes[k]);
                            }
                        }
                    }
                }
                Ok(format!("{} faces, {} values, {} padding bytes, {} problems", nd.mesh.faces.len(), rep.values, rep.uncovered_nonzero, v.len()))
            })();
            match r {
                Ok(s) => {
                    if verbose {
                        println!("ok   {} {i} {name}: {s}", p.display());
                    }
                }
                Err(e) => {
                    bad += 1;
                    println!("FAIL {} {i} {name}: {e}", p.display());
                }
            }
        }
    }
    println!("{n} nav mesh tagfiles, {bad} failed; {faces} faces, {values} values re-encoded, {uncovered} uncovered non-zero bytes, {problems} validation problems; AABB trees rebuilt identical: {trees_same}/{trees}");
    if bad > 0 {
        return Err(format!("{bad} failures").into());
    }
    Ok(())
}
