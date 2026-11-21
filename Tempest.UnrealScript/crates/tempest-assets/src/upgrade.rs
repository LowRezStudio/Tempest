//! Upgrading a mod to deltas (`mod.upgrade`, `upk mod upgrade`, the CLI's
//! `tempest mod upgrade`): a `.tempest` (or `.zip`) package, a mod folder, or
//! a bare `.upk` / `.pck` / `.dat` of the first mod format is unpacked, and
//! every game file it changes, shipped whole (`files/`) or as a delta made by
//! an older builder (`deltas/`), is made again as a delta of today's format.
//! This module unpacks the mod and says what each file is, rebuilds the file
//! a delta stands for, and compares two versions of a game file; the server's
//! `mod_upgrade` runs the cooker over the result and checks every new delta.

use std::io::Read;
use std::path::{Path, PathBuf};

use tempest_upk::PackageFile;

use crate::delta::{apply_with, Delta, Phase};
use crate::edit::Editor;
use crate::reflect::ClassDb;

/// What a file of the unpacked mod is.
#[derive(Clone, Debug)]
pub enum Entry {
    /// `files/<game path>` of a package, marshal or Wwise file the game has:
    /// a candidate for a delta.
    Whole { rel: String, target: PathBuf },
    /// `deltas/<game path>.tdelta` of a file the game has: made again.
    Delta { rel: String, target: PathBuf },
    /// Anything else (code, scripts, readme, icon, new files, shared
    /// objects): carried over as it is.
    Other { rel: String },
}

impl Entry {
    pub fn rel(&self) -> &str {
        match self {
            Entry::Whole { rel, .. } | Entry::Delta { rel, .. } | Entry::Other { rel } => rel,
        }
    }
}

/// An unpacked mod: its folder, its manifest and its files.
pub struct Unpacked {
    pub dir: PathBuf,
    pub manifest: String,
    pub entries: Vec<Entry>,
    /// The input was a bare file of the first mod format (a manifest was made for it).
    pub legacy: bool,
}

/// Files a package carries for its signature: they no longer match once the
/// contents change, so the upgraded package is sealed again.
const SEAL: &[&str] = &["CHECKSUMS", "CHECKSUMS.asc", "SIGNER.pub"];

/// The kind the cooker names a game file by (`upk`, `mcts`, `pck`), from its extension.
pub fn kind_of(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_string_lossy().to_ascii_lowercase().as_str() {
        "upk" => Some("upk"),
        "dat" => Some("mcts"),
        "pck" => Some("pck"),
        _ => None,
    }
}

/// The game's file at `rel` (`/`-separated, under the game root), matched
/// without regard to case as the launcher's installer does.
pub fn game_file(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut at = root.to_path_buf();
    // The folder's own spelling, also on file systems that ignore case.
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        let names: Vec<String> = std::fs::read_dir(&at).ok()?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
        let name = names.iter().find(|n| n.as_str() == part).or_else(|| names.iter().find(|n| n.eq_ignore_ascii_case(part)))?;
        at = at.join(name);
    }
    at.is_file().then_some(at)
}

/// Unpack `input` into `dir` (emptied first) and sort its files out against
/// the game installed at `root`.
pub fn unpack(input: &Path, dir: &Path, root: &Path) -> Result<Unpacked, String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let lower = input.to_string_lossy().to_ascii_lowercase();
    let mut legacy = false;
    if input.is_dir() {
        copy_tree(input, dir).map_err(|e| format!("{}: {e}", input.display()))?;
    } else if lower.ends_with(".tempest") || lower.ends_with(".zip") {
        extract(input, dir)?;
    } else if kind_of(input).is_some() {
        // The first mod format: the file replaces the game's file of that name.
        let name = input.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let target = find_by_name(root, &name).ok_or_else(|| format!("the game has no file named {name}: open the game folder this mod was made for"))?;
        let rel = relative(root, &target);
        let to = dir.join("files").join(&rel);
        std::fs::create_dir_all(to.parent().unwrap_or(dir)).map_err(|e| e.to_string())?;
        std::fs::copy(input, &to).map_err(|e| format!("{}: {e}", input.display()))?;
        let stem = input.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "mod".into());
        let id: String = stem.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>().trim_matches('-').to_string();
        std::fs::write(dir.join("manifest.toml"), format!("[mod]\nid = \"{id}\"\nname = \"{}\"\nversion = \"1.0.0\"\n", stem.replace('"', "'"))).map_err(|e| e.to_string())?;
        legacy = true;
    } else {
        return Err(format!("{} is not a mod: give a .tempest or .zip package, a mod folder, or a .upk, .pck or .dat file", input.display()));
    }
    // A package zipped with its folder: the manifest one level down.
    if !dir.join("manifest.toml").exists() {
        let inner: Vec<PathBuf> = std::fs::read_dir(dir).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.join("manifest.toml").exists()).collect();
        if let [only] = inner.as_slice() {
            let tmp = dir.with_extension("inner");
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::rename(only, &tmp).map_err(|e| e.to_string())?;
            std::fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, dir).map_err(|e| e.to_string())?;
        }
    }
    let manifest = std::fs::read_to_string(dir.join("manifest.toml")).map_err(|_| format!("{} has no manifest.toml: it is not a mod package", input.display()))?;
    let mut entries = Vec::new();
    for file in walk(dir) {
        let rel = relative(dir, &file);
        if rel.eq_ignore_ascii_case("manifest.toml") || SEAL.iter().any(|s| rel.eq_ignore_ascii_case(s)) {
            continue;
        }
        let lower = rel.to_ascii_lowercase();
        let entry = if let Some(game) = lower.strip_prefix("files/").map(|_| &rel["files/".len()..]) {
            match (kind_of(Path::new(game)), game_file(root, game)) {
                (Some(_), Some(target)) => Entry::Whole { rel, target },
                _ => Entry::Other { rel },
            }
        } else if let Some(game) = lower.strip_prefix("deltas/").filter(|g| g.ends_with(".tdelta")).map(|g| &rel["deltas/".len().."deltas/".len() + g.len() - ".tdelta".len()]) {
            match game_file(root, game) {
                Some(target) => Entry::Delta { rel, target },
                None => Entry::Other { rel },
            }
        } else {
            Entry::Other { rel }
        };
        entries.push(entry);
    }
    entries.sort_by(|a, b| a.rel().cmp(b.rel()));
    Ok(Unpacked { dir: dir.to_path_buf(), manifest, entries, legacy })
}

/// The file a delta stands for: `deltas` applied in order to `base` (a game
/// file in its pristine state), written to `out`. Deltas adding classes the
/// class database does not have cannot be rebuilt this way (they need the
/// launcher's two passes): an error says so.
pub fn rebuild(db: &ClassDb, base: &Path, deltas: &[Delta], out: &Path) -> Result<Vec<String>, String> {
    let kind = deltas.first().map(|d| d.kind().to_string()).unwrap_or_default();
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut warnings = Vec::new();
    match kind.as_str() {
        "mcts" => {
            use crate::mcts::{delta as mdelta, parse_obscured, write_obscured, Tokens};
            let tokens = Tokens::for_dir(base.parent().unwrap_or(Path::new(".")));
            let mut m = parse_obscured(&std::fs::read(base).map_err(|e| e.to_string())?, &tokens).map_err(|e| e.to_string())?;
            for d in deltas {
                warnings.extend(mdelta::apply(&tokens, &mut m, d).map_err(|e| e.to_string())?.warnings);
            }
            std::fs::write(out, write_obscured(&m, &tokens)).map_err(|e| e.to_string())?;
        }
        "pck" => {
            use crate::wwise::{akpk, delta as wdelta};
            let mut p = akpk::Package::parse(&std::fs::read(base).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            for d in deltas {
                warnings.extend(wdelta::apply(&mut p, d).map_err(|e| e.to_string())?.warnings);
            }
            std::fs::write(out, p.to_bytes()).map_err(|e| e.to_string())?;
        }
        _ => {
            if let Some(c) = deltas.iter().flat_map(|d| d.added_classes()).find(|c| db.class(c).is_none()) {
                return Err(format!("it adds the class {c}, which only the launcher's two-pass apply can rebuild"));
            }
            let mut ed = Editor::open(db, base).map_err(|e| e.to_string())?;
            let original = deltas.iter().any(|d| !d.from.is_empty()).then(|| ed.pkg.clone());
            for d in deltas {
                let r = apply_with(db, &mut ed, d, Phase::All, original.as_ref()).map_err(|e| e.to_string())?;
                warnings.extend(r.warnings);
            }
            ed.save(out).map_err(|e| e.to_string())?;
        }
    }
    Ok(warnings)
}

/// How `result` differs from `expected` (two versions of one game file,
/// compared object by object, row by row or entry by entry): empty when they
/// are the same.
pub fn differences(db: &ClassDb, expected: &Path, result: &Path) -> Result<Vec<String>, String> {
    match kind_of(expected) {
        Some("mcts") => {
            use crate::mcts::{delta as mdelta, parse_obscured, Tokens};
            let tokens = Tokens::for_dir(result.parent().unwrap_or(Path::new(".")));
            let load = |p: &Path| parse_obscured(&std::fs::read(p).map_err(|e| e.to_string())?, &tokens).map_err(|e| e.to_string());
            Ok(mdelta::compare(&tokens, &load(expected)?, &load(result)?))
        }
        Some("pck") => {
            use crate::wwise::{akpk, delta as wdelta};
            let load = |p: &Path| akpk::Package::parse(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string());
            Ok(wdelta::compare(&load(expected)?, &load(result)?))
        }
        _ => {
            let load = |p: &Path| PackageFile::open(p).and_then(|f| f.load()).map_err(|e| e.to_string());
            Ok(crate::delta::compare(db, &load(expected)?, &load(result)?))
        }
    }
}

/// The deflate size of a file, as a zip entry of a `.tempest` stores it.
pub fn zipped_size(path: &Path) -> std::io::Result<u64> {
    use std::io::Write;
    struct Count(u64);
    impl Write for Count {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0 += b.len() as u64;
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut z = flate2::write::DeflateEncoder::new(Count(0), flate2::Compression::default());
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        z.write_all(&buf[..n])?;
    }
    Ok(z.finish()?.0)
}

/// A game file by name, where the first mod format's installer looks for it:
/// the cooked folder and its language subfolders, then the localization folders.
fn find_by_name(root: &Path, name: &str) -> Option<PathBuf> {
    let cooked = root.join("ChaosGame").join("CookedPCConsole");
    let lower = name.to_ascii_lowercase();
    let in_dir = |d: &Path| std::fs::read_dir(d).ok()?.filter_map(|e| e.ok()).find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name)).map(|e| e.path());
    if let Some(p) = in_dir(&cooked) {
        return Some(p);
    }
    for sub in std::fs::read_dir(&cooked).ok()?.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
        if let Some(p) = in_dir(&sub.path()) {
            return Some(p);
        }
    }
    if lower.starts_with("lang_") && lower.ends_with(".dat") {
        let lang = lower.trim_start_matches("lang_").trim_end_matches(".dat").to_ascii_uppercase();
        return game_file(root, &format!("ChaosGame/Localization/{lang}/{name}"));
    }
    None
}

/// `path` under `base`, `/`-separated.
fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base).unwrap_or(path).components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/")
}

/// Every file under `dir`, dot files left out.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().filter_map(|e| e.ok()) {
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    for file in walk(from) {
        let dest = to.join(file.strip_prefix(from).unwrap_or(&file));
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&file, dest)?;
    }
    Ok(())
}

/// Extract a zip into `dir`, refusing entries that would land outside it.
fn extract(zip_path: &Path, dir: &Path) -> Result<(), String> {
    let f = std::fs::File::open(zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("{} is not a zip package: {e}", zip_path.display()))?;
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).map_err(|e| e.to_string())?;
        if e.is_dir() {
            continue;
        }
        let Some(name) = e.enclosed_name() else {
            return Err(format!("{} has an entry outside its folder ({})", zip_path.display(), e.name()));
        };
        let out = dir.join(name);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut w = std::fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        std::io::copy(&mut e, &mut w).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_files_are_found_whatever_their_case() {
        let root = std::env::temp_dir().join(format!("tempest-upgrade-test-{}", std::process::id()));
        let cooked = root.join("ChaosGame").join("CookedPCConsole");
        std::fs::create_dir_all(&cooked).unwrap();
        std::fs::write(cooked.join("Maeve_ASSETS_SF.upk"), b"x").unwrap();
        assert_eq!(game_file(&root, "chaosgame/cookedpcconsole/maeve_assets_sf.upk"), Some(cooked.join("Maeve_ASSETS_SF.upk")));
        assert_eq!(game_file(&root, "ChaosGame/CookedPCConsole/missing.upk"), None);
        assert_eq!(find_by_name(&root, "MAEVE_assets_sf.upk"), Some(cooked.join("Maeve_ASSETS_SF.upk")));
        // A package: files/ of a known game file becomes a delta candidate, the rest is carried over.
        let pkg = root.join("mod");
        std::fs::create_dir_all(pkg.join("files/chaosgame/cookedpcconsole")).unwrap();
        std::fs::create_dir_all(pkg.join("dlls/64")).unwrap();
        std::fs::write(pkg.join("manifest.toml"), "[mod]\nid = \"m\"\nname = \"M\"\nversion = \"1.0.0\"\n").unwrap();
        std::fs::write(pkg.join("files/chaosgame/cookedpcconsole/maeve_assets_sf.upk"), b"y").unwrap();
        std::fs::write(pkg.join("files/chaosgame/cookedpcconsole/New.upk"), b"z").unwrap();
        std::fs::write(pkg.join("dlls/64/a.dll"), b"d").unwrap();
        std::fs::write(pkg.join("CHECKSUMS"), b"c").unwrap();
        let u = unpack(&pkg, &root.join("work"), &root).unwrap();
        let kinds: Vec<(String, &str)> = u.entries.iter().map(|e| (e.rel().to_string(), match e { Entry::Whole { .. } => "whole", Entry::Delta { .. } => "delta", Entry::Other { .. } => "other" })).collect();
        assert_eq!(kinds, vec![("dlls/64/a.dll".to_string(), "other"), ("files/chaosgame/cookedpcconsole/New.upk".to_string(), "other"), ("files/chaosgame/cookedpcconsole/maeve_assets_sf.upk".to_string(), "whole")]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
