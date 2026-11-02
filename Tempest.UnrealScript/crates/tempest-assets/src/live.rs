//! What the hot reload mod (`upk-hotreload`) needs to update a running game
//! exactly. For every package with movies it records the state the game
//! loaded: each movie's `RawData` hash (FNV-1a 64, which the mod computes over
//! the bytes in memory too) and the external images it names, and the
//! canonical hash of every other object. States live in a folder, reused
//! while the file's size and modification time match. A later version of a
//! file is compared with its recorded state: the movies whose bytes changed
//! (written out for the mod to swap in), the other packages' copies of those
//! movies (memory holds the copy of whichever package loaded first), the
//! textures and other objects that changed (the game only sees those after a
//! restart).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Instant, UNIX_EPOCH};

use serde_json::{json, Value as J};
use tempest_upk::{Package, PackageFile, PackageIndex};

use crate::delta;
use crate::object::package_layout;
use crate::reflect::ClassDb;
use crate::swf;

type Res<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const VERSION: u64 = 3;
const INDEX: &str = "index.json";
/// Paths unique to a package that the mod checks for in memory: one of them
/// loaded means the package was.
const UNIQUE_SAMPLES: usize = 12;
const OTHER_SAMPLES: usize = 20;

pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn hex64(h: u64) -> String {
    format!("{h:016x}")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Size and modification time; a recorded state is reused while both match.
pub fn stamp(path: &Path) -> Option<String> {
    let m = std::fs::metadata(path).ok()?;
    let t = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!("{}-{}", m.len(), t.as_nanos()))
}

fn is_movie_class(class: &str) -> bool {
    class.eq_ignore_ascii_case("SwfMovie") || class.eq_ignore_ascii_case("FlashMovie")
}

/// A movie, not the class default object `GFxUI.upk` defines.
fn is_movie(pkg: &Package, i: usize) -> bool {
    let idx = PackageIndex::export(i);
    is_movie_class(&pkg.class_name_of(idx)) && !pkg.object_name(idx).starts_with("Default__")
}

pub struct Movie {
    pub hash: u64,
    pub size: usize,
    /// Lower-case file stems of the external images it names (`Foo.tga` → `foo`).
    pub images: Vec<String>,
    /// The movie itself; empty for a state read back from the folder.
    pub bytes: Vec<u8>,
}

pub struct Object {
    pub class: String,
    /// `delta::canonical` hash.
    pub canonical: String,
    /// FNV-1a 64 of the export's table entry and payload: equal under compatible tables means
    /// canonically equal, so only objects whose fingerprint changed are parsed.
    pub fingerprint: u64,
}

/// Running hashes of the name, import and export tables, so a later version can tell whether it
/// only appended to them (the Studio's editor and the launcher's delta resolver do): then every
/// index an old payload holds still means the same thing.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Tables {
    pub names: (usize, u64),
    pub imports: (usize, u64),
    pub exports: (usize, u64),
}

pub struct State {
    pub file: String,
    pub stamp: String,
    pub tables: Tables,
    /// Lower-case object path → movie.
    pub movies: BTreeMap<String, Movie>,
    /// Lower-case object path → every other object.
    pub objects: BTreeMap<String, Object>,
}

impl State {
    fn to_json(&self) -> J {
        let movies: serde_json::Map<String, J> =
            self.movies.iter().map(|(p, m)| (p.clone(), json!([hex64(m.hash), m.size, m.images]))).collect();
        let objects: serde_json::Map<String, J> =
            self.objects.iter().map(|(p, o)| (p.clone(), json!([o.class, o.canonical, hex64(o.fingerprint)]))).collect();
        let t = |(n, h): (usize, u64)| json!([n, hex64(h)]);
        json!({
            "format": "upk-live",
            "version": VERSION,
            "file": self.file,
            "stamp": self.stamp,
            "tables": [t(self.tables.names), t(self.tables.imports), t(self.tables.exports)],
            "movies": movies,
            "objects": objects,
        })
    }

    fn from_json(v: &J) -> Option<State> {
        if v["format"] != "upk-live" || v["version"].as_u64() != Some(VERSION) {
            return None;
        }
        let hex = |s: &J| u64::from_str_radix(s.as_str()?, 16).ok();
        let mut movies = BTreeMap::new();
        for (p, m) in v["movies"].as_object()? {
            let images = m[2].as_array()?.iter().filter_map(|s| s.as_str().map(str::to_string)).collect();
            movies.insert(p.clone(), Movie { hash: hex(&m[0])?, size: m[1].as_u64()? as usize, images, bytes: Vec::new() });
        }
        let mut objects = BTreeMap::new();
        for (p, o) in v["objects"].as_object()? {
            objects.insert(p.clone(), Object { class: o[0].as_str()?.to_string(), canonical: o[1].as_str()?.to_string(), fingerprint: hex(&o[2])? });
        }
        let t = |i: usize| Some((v["tables"][i][0].as_u64()? as usize, hex(&v["tables"][i][1])?));
        let tables = Tables { names: t(0)?, imports: t(1)?, exports: t(2)? };
        Some(State { file: v["file"].as_str()?.to_string(), stamp: v["stamp"].as_str()?.to_string(), tables, movies, objects })
    }
}

/// FNV-1a 64 continued over `bytes`.
fn fnv_more(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// Hash after each entry (index 0: the empty prefix).
fn running(entries: impl Iterator<Item = Vec<u8>>) -> Vec<u64> {
    let mut out = vec![FNV_BASIS];
    let mut h = FNV_BASIS;
    for e in entries {
        h = fnv_more(h, &e);
        h = fnv_more(h, &[0xff]);
        out.push(h);
    }
    out
}

fn fname_bytes(pkg: &Package, n: tempest_upk::io::FName) -> Vec<u8> {
    let mut v = pkg.name_str(n.index).to_ascii_lowercase().into_bytes();
    v.extend_from_slice(&n.number.to_le_bytes());
    v
}

struct TableRuns {
    names: Vec<u64>,
    imports: Vec<u64>,
    exports: Vec<u64>,
}

impl TableRuns {
    fn of(pkg: &Package) -> TableRuns {
        let names = running(pkg.names.iter().map(|n| n.name.to_ascii_lowercase().into_bytes()));
        let imports = running(pkg.imports.iter().map(|i| {
            let mut v = fname_bytes(pkg, i.class_package);
            v.extend(fname_bytes(pkg, i.class_name));
            v.extend_from_slice(&i.outer.0.to_le_bytes());
            v.extend(fname_bytes(pkg, i.object_name));
            v
        }));
        let exports = running(pkg.exports.iter().map(|e| {
            let mut v = fname_bytes(pkg, e.object_name);
            for i in [e.class, e.super_, e.outer, e.archetype] {
                v.extend_from_slice(&i.0.to_le_bytes());
            }
            v
        }));
        TableRuns { names, imports, exports }
    }

    fn tables(&self) -> Tables {
        let last = |r: &Vec<u64>| (r.len() - 1, *r.last().unwrap());
        Tables { names: last(&self.names), imports: last(&self.imports), exports: last(&self.exports) }
    }

    /// Whether these tables begin with the ones `old` describes.
    fn extend(&self, old: &Tables) -> bool {
        let starts = |r: &Vec<u64>, (n, h): (usize, u64)| r.get(n) == Some(&h);
        starts(&self.names, old.names) && starts(&self.imports, old.imports) && starts(&self.exports, old.exports)
    }
}

fn fingerprint(pkg: &Package, i: usize) -> u64 {
    let e = &pkg.exports[i];
    let mut head = Vec::with_capacity(64);
    for x in [e.class.0, e.super_.0, e.archetype.0, e.outer.0] {
        head.extend_from_slice(&x.to_le_bytes());
    }
    head.extend_from_slice(&e.object_flags.to_le_bytes());
    head.extend_from_slice(&e.export_flags.to_le_bytes());
    head.extend_from_slice(&e.package_flags.to_le_bytes());
    for c in &e.generation_net_object_count {
        head.extend_from_slice(&c.to_le_bytes());
    }
    fnv_more(fnv_more(FNV_BASIS, &head), &pkg.export_data[i])
}

/// `SwfMovie.RawData` straight from the tagged properties: no class database needed.
fn raw_data(pkg: &Package, i: usize) -> Option<Vec<u8>> {
    use tempest_unrealscript::objects::{read_object_header, HeaderOptions};
    let mut r = tempest_upk::io::Reader::new(&pkg.export_data[i]);
    let h = read_object_header(&mut r, pkg, HeaderOptions { object_flags: pkg.exports[i].object_flags, ..Default::default() }).ok()?;
    let p = h.properties.iter().find(|p| p.array_index == 0 && pkg.name_str(p.name.index).eq_ignore_ascii_case("RawData"))?;
    let n = u32::from_le_bytes(p.raw.get(..4)?.try_into().ok()?) as usize;
    p.raw.get(4..4 + n).map(<[u8]>::to_vec)
}

/// The game's class database, loaded the first time an object has to be parsed.
pub struct LazyDb {
    dir: PathBuf,
    db: std::sync::OnceLock<Result<ClassDb, String>>,
}

impl LazyDb {
    pub fn new(dir: &Path) -> LazyDb {
        LazyDb { dir: dir.to_path_buf(), db: std::sync::OnceLock::new() }
    }

    fn get(&self) -> Res<&ClassDb> {
        match self.db.get_or_init(|| ClassDb::load_dir(&self.dir).map_err(|e| e.to_string())) {
            Ok(db) => Ok(db),
            Err(e) => Err(e.clone().into()),
        }
    }

    pub fn loaded(&self) -> bool {
        self.db.get().is_some()
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn file_stem(file: &str) -> &str {
    file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file)
}

fn state_path(dir: &Path, file: &str) -> PathBuf {
    dir.join(format!("{}.json", file.to_ascii_lowercase()))
}

fn read_json(path: &Path) -> Option<J> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Written whole and then renamed over the old file, so an interrupted write never leaves half a state.
fn write_json(path: &Path, v: &J) -> Res<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(v)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Lower-case file name → (stamp, number of movies).
struct Index {
    files: BTreeMap<String, (String, u64)>,
}

impl Index {
    fn read(dir: &Path) -> Index {
        let mut files = BTreeMap::new();
        if let Some(v) = read_json(&dir.join(INDEX)) {
            if v["format"] == "upk-live-index" && v["version"].as_u64() == Some(VERSION) {
                for (name, e) in v["files"].as_object().into_iter().flatten() {
                    if let (Some(s), Some(n)) = (e["stamp"].as_str(), e["movies"].as_u64()) {
                        files.insert(name.clone(), (s.to_string(), n));
                    }
                }
            }
        }
        Index { files }
    }

    fn write(&self, dir: &Path) -> Res<()> {
        let files: serde_json::Map<String, J> =
            self.files.iter().map(|(k, (s, n))| (k.clone(), json!({ "stamp": s, "movies": n }))).collect();
        write_json(&dir.join(INDEX), &json!({ "format": "upk-live-index", "version": VERSION, "files": files }))
    }
}

/// How many movies a package holds, from its tables only.
fn count_movies(path: &Path) -> Res<u64> {
    let pf = PackageFile::open(path)?;
    let pkg = pf.tables();
    Ok((0..pkg.exports.len()).filter(|&i| is_movie(pkg, i)).count() as u64)
}

/// The state of a package file as it is now; `None` when it holds no movie.
/// Read into memory (not mapped), so a writer is never blocked for long.
/// Objects whose fingerprint matches `previous` (and whose tables only
/// grew) keep the recorded hashes; only the others are parsed.
pub fn read_state(db: &LazyDb, path: &Path, previous: Option<&State>) -> Res<Option<State>> {
    let before = stamp(path).ok_or("cannot read the file's size and time")?;
    let pkg = Package::load(path)?;
    if stamp(path).as_deref() != Some(before.as_str()) {
        return Err("the file changed while it was read".into());
    }
    let n = pkg.exports.len();
    if !(0..n).any(|i| is_movie(&pkg, i)) {
        return Ok(None);
    }
    let runs = TableRuns::of(&pkg);
    let debug = std::env::var_os("UPK_LIVE_DEBUG").is_some();
    if debug {
        if let Some(p) = previous {
            eprintln!("[live] {}: tables extend the record: {}", path.display(), runs.extend(&p.tables));
        }
    }
    let previous = previous.filter(|p| runs.extend(&p.tables));
    let layout = package_layout(&pkg);
    let mut movies = BTreeMap::new();
    let mut objects = BTreeMap::new();
    for i in 0..n {
        let idx = PackageIndex::export(i);
        let path = pkg.object_path(idx).to_ascii_lowercase();
        if is_movie(&pkg, i) {
            let Some(bytes) = raw_data(&pkg, i) else { continue };
            let hash = fnv64(&bytes);
            let images = match previous.and_then(|p| p.movies.get(&path)).filter(|m| m.hash == hash) {
                Some(m) => m.images.clone(),
                None => swf::Movie::parse(&bytes).map(|m| image_stems(&m)).unwrap_or_default(),
            };
            movies.insert(path, Movie { hash, size: bytes.len(), images, bytes });
            continue;
        }
        let class = pkg.class_name_of(idx).to_ascii_lowercase();
        // Placeholders all share the path "none".
        let path = if objects.contains_key(&path) { format!("{path}#{i}") } else { path };
        let fp = fingerprint(&pkg, i);
        if let Some(o) = previous.and_then(|p| p.objects.get(&path)).filter(|o| o.fingerprint == fp && o.class == class) {
            objects.insert(path, Object { class, canonical: o.canonical.clone(), fingerprint: fp });
        } else {
            if debug && previous.is_some() {
                eprintln!("[live]   parsed {path} ({class})");
            }
            // A placeholder (a deleted export) has no canonical hash; it is kept with an empty one so
            // its fingerprint spares the parse next time.
            let canonical = delta::canonical(db.get()?, &pkg, layout, i).map(|h| hex(&h)).unwrap_or_default();
            objects.insert(path, Object { class, canonical, fingerprint: fp });
        }
    }
    Ok(Some(State { file: file_name(path), stamp: before, tables: runs.tables(), movies, objects }))
}

fn image_stems(m: &swf::Movie) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in &m.tags {
        if t.code != 1009 && t.code != 1001 {
            continue;
        }
        let Some(img) = swf::parse_external_image(t.code, &t.data) else { continue };
        let file = img.file_name.rsplit(['/', '\\']).next().unwrap_or(&img.file_name);
        let stem = file_stem(file).to_ascii_lowercase();
        if !stem.is_empty() && !out.contains(&stem) {
            out.push(stem);
        }
    }
    out
}

/// Runs `f` over `items` on up to `threads` threads, results in input order.
fn parallel<T: Sync, R: Send>(items: &[T], threads: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..threads.clamp(1, items.len().max(1)) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(i) else { break };
                let r = f(item);
                out.lock().unwrap()[i] = Some(r);
            });
        }
    });
    out.into_inner().unwrap().into_iter().map(|r| r.expect("every item ran")).collect()
}

fn threads(cap: usize) -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).min(cap)
}

fn load_states(dir: &Path, index: &Index) -> HashMap<String, State> {
    index
        .files
        .iter()
        .filter(|(_, (_, n))| *n > 0)
        .filter_map(|(file, _)| Some((file.clone(), State::from_json(&read_json(&state_path(dir, file))?)?)))
        .collect()
}

/// For each movie package of `states`, paths no other package holds.
fn unique_samples(states: &HashMap<String, State>) -> J {
    let mut count: HashMap<&str, usize> = HashMap::new();
    for s in states.values() {
        for p in s.movies.keys() {
            *count.entry(p.as_str()).or_default() += 1;
        }
    }
    let mut out = serde_json::Map::new();
    for s in states.values() {
        let unique: Vec<&String> = s.movies.keys().filter(|p| count.get(p.as_str()) == Some(&1)).take(UNIQUE_SAMPLES).collect();
        out.insert(file_stem(&s.file).to_string(), json!({ "movies": s.movies.len(), "unique": unique }));
    }
    J::Object(out)
}

/// `upk live baseline`: record the state of every movie package of `cooked`
/// whose file changed since it was last recorded (all of them the first
/// time). The class database is only loaded when one has to be recorded.
pub fn baseline(cooked: &Path, state_dir: &Path) -> Res<J> {
    let started = Instant::now();
    std::fs::create_dir_all(state_dir)?;
    let mut index = Index::read(state_dir);
    let mut files: Vec<(String, PathBuf, String)> = Vec::new();
    for e in std::fs::read_dir(cooked)? {
        let path = e?.path();
        let name = file_name(&path);
        if !name.to_ascii_lowercase().ends_with(".upk") {
            continue;
        }
        if let Some(s) = stamp(&path) {
            files.push((name.to_ascii_lowercase(), path, s));
        }
    }
    let present: HashSet<&str> = files.iter().map(|(k, _, _)| k.as_str()).collect();
    let gone: Vec<String> = index.files.keys().filter(|k| !present.contains(k.as_str())).cloned().collect();
    for k in &gone {
        index.files.remove(k);
        let _ = std::fs::remove_file(state_path(state_dir, k));
    }
    let old = load_states(state_dir, &index);
    // A state that does not read back (deleted, or written by another version) is recorded again.
    let stale: Vec<&(String, PathBuf, String)> = files
        .iter()
        .filter(|(k, _, s)| match index.files.get(k) {
            Some((stamp, n)) => stamp != s || (*n > 0 && !old.contains_key(k)),
            None => true,
        })
        .collect();
    let counts = parallel(&stale, threads(4), |(_, path, _)| count_movies(path).unwrap_or(0));
    let mut record: Vec<&(String, PathBuf, String)> = Vec::new();
    for (f, n) in stale.iter().zip(&counts) {
        if *n > 0 {
            record.push(f);
        } else {
            index.files.insert(f.0.clone(), (f.2.clone(), 0));
            let _ = std::fs::remove_file(state_path(state_dir, &f.0));
        }
    }
    let mut errors = Vec::new();
    let mut refreshed = Vec::new();
    if !record.is_empty() {
        let db = LazyDb::new(cooked);
        let states = parallel(&record, threads(3), |(key, path, _)| read_state(&db, path, old.get(key)));
        for ((key, path, s), st) in record.iter().map(|f| (&f.0, &f.1, &f.2)).zip(states) {
            match st {
                Ok(Some(st)) if &st.stamp == s => {
                    write_json(&state_path(state_dir, key), &st.to_json())?;
                    index.files.insert(key.clone(), (st.stamp.clone(), st.movies.len() as u64));
                    refreshed.push(file_name(path));
                }
                Ok(Some(_)) => errors.push(json!({ "file": file_name(path), "error": "the file changed while it was read" })),
                Ok(None) => {
                    index.files.insert(key.clone(), (s.clone(), 0));
                }
                Err(e) => errors.push(json!({ "file": file_name(path), "error": e.to_string() })),
            }
        }
    }
    index.write(state_dir)?;
    let states = load_states(state_dir, &index);
    Ok(json!({
        "scanned": stale.len(),
        "refreshed": refreshed,
        "errors": errors,
        "packages": unique_samples(&states),
        "ms": started.elapsed().as_millis() as u64,
    }))
}

/// `upk live changes`: what each of `packages` changed against its recorded
/// state, then record the new one. Changed movies are written to
/// `<out>/<package>/<path>.gfx`. A package without a recorded state lists
/// every movie with `old: null` (the mod then only trusts movies no other
/// package holds).
pub fn changes(req: &J) -> Res<J> {
    let started = Instant::now();
    let state_dir = PathBuf::from(req["state"].as_str().ok_or("missing state")?);
    let out_dir = PathBuf::from(req["out"].as_str().ok_or("missing out")?);
    let packages: Vec<PathBuf> =
        req["packages"].as_array().ok_or("missing packages")?.iter().filter_map(|p| p.as_str().map(PathBuf::from)).collect();
    let scripts = match req["scripts"].as_str() {
        Some(s) => PathBuf::from(s),
        None => packages.first().and_then(|p| p.parent()).map(Path::to_path_buf).ok_or("no package given")?,
    };
    std::fs::create_dir_all(&state_dir)?;
    let mut index = Index::read(&state_dir);
    let old_states = load_states(&state_dir, &index);

    let mut counts = Vec::new();
    for p in &packages {
        counts.push(count_movies(p));
    }
    let with_movies: Vec<&PathBuf> = packages.iter().zip(&counts).filter(|(_, n)| matches!(n, Ok(n) if *n > 0)).map(|(p, _)| p).collect();
    let mut new_states: HashMap<String, Res<Option<State>>> = HashMap::new();
    let db = LazyDb::new(&scripts);
    if !with_movies.is_empty() {
        let read = parallel(&with_movies, threads(4), |p| read_state(&db, p, old_states.get(&file_name(p).to_ascii_lowercase())));
        for (p, st) in with_movies.iter().zip(read) {
            new_states.insert(file_name(p).to_ascii_lowercase(), st);
        }
    }

    // Path → every other package's recorded hash of it.
    let mut locations: HashMap<&str, Vec<(&str, u64)>> = HashMap::new();
    for s in old_states.values() {
        for (p, m) in &s.movies {
            locations.entry(p.as_str()).or_default().push((file_stem(&s.file), m.hash));
        }
    }
    let copies_of = |path: &str, own: &str| -> J {
        let mut m = serde_json::Map::new();
        for (pkg, h) in locations.get(path).into_iter().flatten() {
            if !pkg.eq_ignore_ascii_case(own) {
                m.insert(pkg.to_string(), J::String(hex64(*h)));
            }
        }
        J::Object(m)
    };

    let mut results = Vec::new();
    let mut recorded: HashMap<String, State> = HashMap::new();
    for (p, count) in packages.iter().zip(counts) {
        let file = file_name(p);
        let key = file.to_ascii_lowercase();
        let name = file_stem(&file).to_string();
        let count = match count {
            Ok(n) => n,
            Err(e) => {
                results.push(json!({ "name": name, "file": p, "error": e.to_string() }));
                continue;
            }
        };
        if count == 0 {
            if let Some(s) = stamp(p) {
                index.files.insert(key.clone(), (s, 0));
            }
            let _ = std::fs::remove_file(state_path(&state_dir, &key));
            results.push(json!({ "name": name, "file": p, "movies": 0 }));
            continue;
        }
        let new = match new_states.remove(&key) {
            Some(Ok(Some(st))) => st,
            Some(Ok(None)) => {
                results.push(json!({ "name": name, "file": p, "movies": 0 }));
                continue;
            }
            Some(Err(e)) => {
                results.push(json!({ "name": name, "file": p, "error": e.to_string() }));
                continue;
            }
            None => continue,
        };
        let old = old_states.get(&key);
        let dir = out_dir.join(&name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let mut changed = Vec::new();
        for (path, m) in &new.movies {
            let old_hash = old.map(|o| o.movies.get(path).map(|om| om.hash));
            // Recorded and unchanged, or new in this version (nothing in memory to replace).
            if matches!(old_hash, Some(Some(h)) if h == m.hash) || matches!(old_hash, Some(None)) {
                continue;
            }
            let out = dir.join(format!("{path}.gfx"));
            std::fs::write(&out, &m.bytes)?;
            changed.push(json!({
                "path": path,
                "old": old_hash.flatten().map(hex64),
                "new": hex64(m.hash),
                "size": m.size,
                "file": out,
                "copies": copies_of(path, &name),
            }));
        }
        let (added, removed): (Vec<&String>, Vec<&String>) = match old {
            Some(o) => (
                new.movies.keys().filter(|p| !o.movies.contains_key(*p)).collect(),
                o.movies.keys().filter(|p| !new.movies.contains_key(*p)).collect(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        // Texture name → the movies naming it.
        let mut users: HashMap<&str, Vec<&str>> = HashMap::new();
        for (path, m) in &new.movies {
            for img in &m.images {
                users.entry(img.as_str()).or_default().push(path.as_str());
            }
        }
        let mut textures = Vec::new();
        let mut other_count: BTreeMap<&str, usize> = BTreeMap::new();
        let mut other_sample = Vec::new();
        if let Some(o) = old {
            for (path, obj) in &new.objects {
                let class = &obj.class;
                let before = o.objects.get(path);
                if obj.canonical.is_empty() || before.is_some_and(|b| b.canonical == obj.canonical) {
                    continue;
                }
                let leaf = path.rsplit('.').next().unwrap_or(path);
                if class == "texture2d" {
                    if let Some(movies) = users.get(leaf) {
                        textures.push(json!({ "path": path, "added": before.is_none(), "movies": movies }));
                        continue;
                    }
                }
                *other_count.entry(class.as_str()).or_default() += 1;
                if other_sample.len() < OTHER_SAMPLES {
                    other_sample.push(json!({ "path": path, "class": class, "added": before.is_none() }));
                }
            }
            let gone = o.objects.iter().filter(|(p, b)| !b.canonical.is_empty() && !new.objects.contains_key(*p)).count();
            if gone > 0 {
                other_count.insert("removed", gone);
            }
        }
        results.push(json!({
            "name": name,
            "file": p,
            "movies": new.movies.len(),
            "previous": old.is_some(),
            "changed": changed,
            "added": added,
            "removed": removed,
            "textures": textures,
            "other": { "count": other_count.values().sum::<usize>(), "classes": other_count, "sample": other_sample },
        }));
        write_json(&state_path(&state_dir, &key), &new.to_json())?;
        index.files.insert(key.clone(), (new.stamp.clone(), new.movies.len() as u64));
        recorded.insert(key, new);
    }
    index.write(&state_dir)?;
    let parsed = db.loaded();
    let mut all = old_states;
    all.extend(recorded);
    Ok(json!({
        "packages": results,
        "movie_packages": unique_samples(&all),
        "ms": started.elapsed().as_millis() as u64,
        "class_db": parsed,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv64_matches_the_reference_values() {
        assert_eq!(fnv64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn state_round_trips_through_json() {
        let mut movies = BTreeMap::new();
        movies.insert("hud.hud".to_string(), Movie { hash: 0x0123_4567_89ab_cdef, size: 42, images: vec!["hud_i3".into()], bytes: vec![1, 2] });
        let mut objects = BTreeMap::new();
        objects.insert("hud.hud_i3".to_string(), Object { class: "texture2d".into(), canonical: "00ff".into(), fingerprint: u64::MAX });
        let tables = Tables { names: (3, 1), imports: (0, 2), exports: (7, 3) };
        let s = State { file: "GameUI_SF.upk".into(), stamp: "1-2".into(), tables, movies, objects };
        let back = State::from_json(&s.to_json()).unwrap();
        assert_eq!(back.file, "GameUI_SF.upk");
        assert_eq!(back.movies["hud.hud"].hash, 0x0123_4567_89ab_cdef);
        assert_eq!(back.movies["hud.hud"].images, vec!["hud_i3".to_string()]);
        assert_eq!(back.objects["hud.hud_i3"].canonical, "00ff");
        assert_eq!(back.objects["hud.hud_i3"].fingerprint, u64::MAX);
        assert!(back.tables == tables);
    }

    #[test]
    fn tables_that_only_grew_extend_the_old_ones() {
        let a = running(["x", "y"].iter().map(|s| s.as_bytes().to_vec()));
        let b = running(["x", "y", "z"].iter().map(|s| s.as_bytes().to_vec()));
        let c = running(["x", "q", "z"].iter().map(|s| s.as_bytes().to_vec()));
        let old = (a.len() - 1, *a.last().unwrap());
        assert_eq!(b.get(old.0), Some(&old.1));
        assert_ne!(c.get(old.0), Some(&old.1));
    }
}
