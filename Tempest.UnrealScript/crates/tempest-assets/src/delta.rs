//! Package deltas: what a modded package changed relative to the game's own
//! file, as a `.tdelta` the launcher applies over whatever the file is when
//! the mod is enabled. Mods then ship a few kilobytes instead of whole
//! packages, and two mods editing different objects of the same package
//! compose.
//!
//! An object is addressed by its path (`Outer.Inner.Name`), never by its
//! export index: indices shift when two mods add exports. The delta is a
//! small package of its own (the "mini package"): its exports are the added
//! and changed objects with their payloads, its imports are every object
//! those payloads refer to, both the real imports of the edited package and
//! the exports of the base package the delta does not carry (resolved by
//! path at apply time). A JSON header describes the operations, the kind of
//! every reference and the hashes the applier checks for drift. Payloads
//! are remapped through [`crate::visit`], the same walk the editor's
//! cross-package copy uses, so names and references survive the move.
//!
//! What is compared is a *canonical* form of each export, independent of
//! the package's tables: names as text, references as `class|path`
//! descriptors, in order of first appearance, plus the written payload.
//! A recompiled script package therefore only differs where its sources did.
//!
//! **Shared objects** (version 2): a large payload (a mesh, a texture) can
//! live outside the delta, in an *object file* named by the SHA-256 of its
//! bytes. An object file is a one-export package of its own whose names and
//! imports follow the object's content only, so the same object changed in
//! several packages (a seekfree package each) gives the same file, and a mod
//! ships it once: the cooker writes `shared/<hash>.tobj` at the mod root and
//! every delta refers to it by hash. The op keeps its export (table entry) in
//! the mini package; `shared: {hash, refs}` says where its payload is and
//! how the object file's imports resolve (the same descriptors as `refs`).
//! [`Delta::read`] finds object files in a `shared/` folder next to the
//! delta or above it (the installed mod's folder).
//!
//! File layout: `TDLT`, u32 version, u32 json length, json, u32 image
//! length, image (the mini package, uncompressed); version 2 adds u32 count
//! and, per embedded object file, u32 hash length, hash (hex), u32 length,
//! bytes. The image and the object files are LZMA-compressed (an `.xz`
//! stream, recognised by its magic) unless that would not make them
//! smaller. A delta without shared objects or compression is written as
//! version 1.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use md5::{Digest as _, Md5};
use sha2::Sha256;
use serde_json::{json, Value};
use tempest_unrealscript::objects::Layout;
use tempest_upk::io::{Error, FName, PackageIndex, Reader, Result, Writer};
use tempest_upk::{ObjectExport, Package, WriteOptions};

use crate::edit::Editor;
use crate::object::{is_script_class, none_name, package_layout, parse_export, write_parsed, Parsed};
use crate::reflect::ClassDb;
use crate::visit::{visit_parsed, Visitor};

pub const MAGIC: &[u8; 4] = b"TDLT";
/// The newest version this build reads and writes.
pub const VERSION: u32 = 2;
/// Payloads at least this large become shared object files when sharing is on.
pub const SHARE_MIN: usize = 16 * 1024;
/// Extension of a shared object file.
pub const SHARED_EXT: &str = "tobj";

/// Magic of an LZMA (`.xz`) stream: compressed images and object files start with it.
const XZ_MAGIC: &[u8; 6] = b"\xFD7zXZ\x00";

/// Compress with LZMA2 at the strongest preset (dictionary sized to the data,
/// so a reader needs no more memory than the data itself); the bytes as they
/// are when that is not smaller.
pub fn pack(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;
    use xz2::stream::{Check, Filters, LzmaOptions, Stream};
    let packed = (|| -> Option<Vec<u8>> {
        // Preset 9: "extreme" (9e) measured the same size on game data, slower.
        // zstd was measured on every part of a 5-package mod (214 MB): at the
        // same speed (level 19) it is 17 % bigger, at the same size never.
        let mut opts = LzmaOptions::new_preset(9).ok()?;
        // The dictionary covers the whole input (up to 256 MiB): objects far apart
        // in a package are often alike (skins, LODs), and the reader needs no more
        // memory than that.
        opts.dict_size(dict_for(bytes.len()));
        let mut filters = Filters::new();
        filters.lzma2(&opts);
        // One stream on one core: splitting it into blocks to use more cores
        // lost 22 % on a 100 MB image (similar objects far apart no longer
        // match); callers compress several things at once instead.
        let stream = Stream::new_stream_encoder(&filters, Check::None).ok()?;
        let mut enc = xz2::write::XzEncoder::new_stream(Vec::new(), stream);
        enc.write_all(bytes).ok()?;
        enc.finish().ok()
    })();
    match packed {
        Some(p) if p.len() < bytes.len() => p,
        _ => bytes.to_vec(),
    }
}

/// How a mod is compressed: the smallest file (LZMA level 9), or a faster
/// build (zstd level 12). Measured on every part of a 5-package mod (214 MB,
/// 10 cores): LZMA 59.9 MB in 12.4 s, zstd 12 77.9 MB in 1.8 s. Readers tell
/// them apart by their magic, so either installs the same.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Compression {
    #[default]
    Small,
    Fast,
}

impl Compression {
    pub fn parse(s: &str) -> Compression {
        if s.eq_ignore_ascii_case("fast") { Compression::Fast } else { Compression::Small }
    }
}

/// Magic of a zstd frame: what [`Compression::Fast`] writes.
const ZSTD_MAGIC: &[u8; 4] = b"\x28\xB5\x2F\xFD";

/// zstd level 12 with long-distance matching (window up to 128 MiB); the bytes
/// as they are when that is not smaller.
fn pack_fast(bytes: &[u8]) -> Vec<u8> {
    use zstd::zstd_safe::CParameter;
    let window = (usize::BITS - bytes.len().max(1 << 20).leading_zeros()).min(27);
    let packed = (|| -> Option<Vec<u8>> {
        let mut c = zstd::bulk::Compressor::new(12).ok()?;
        c.set_parameter(CParameter::WindowLog(window)).ok()?;
        c.set_parameter(CParameter::EnableLongDistanceMatching(true)).ok()?;
        c.compress(bytes).ok()
    })();
    match packed {
        Some(p) if p.len() < bytes.len() => p,
        _ => bytes.to_vec(),
    }
}

/// [`pack`] or [`pack_fast`].
pub fn pack_with(bytes: &[u8], c: Compression) -> Vec<u8> {
    match c {
        Compression::Small => pack(bytes),
        Compression::Fast => pack_fast(bytes),
    }
}

/// LZMA dictionary for `len` bytes: the whole input (rounded up to 1 MiB,
/// at most 256 MiB), so data far apart still matches.
fn dict_for(len: usize) -> u32 {
    (len.max(4096).div_ceil(1 << 20) * (1 << 20)).min(256 << 20) as u32
}

/// Memory an LZMA encoder takes for `len` bytes (about 12 times its dictionary at preset 9).
fn pack_memory(len: usize) -> usize {
    dict_for(len) as usize * 12 + (64 << 20)
}

/// [`pack`] over many inputs on every core, biggest first, never running more
/// encoders at once than fit in a memory budget (an encoder of a 100 MB input
/// takes over a gigabyte).
pub fn pack_all(inputs: &[&[u8]]) -> Vec<Vec<u8>> {
    pack_all_with(&inputs.iter().map(|b| (*b, Compression::Small)).collect::<Vec<_>>())
}

/// [`pack_all`], each input with its own compression.
pub fn pack_all_with(inputs: &[(&[u8], Compression)]) -> Vec<Vec<u8>> {
    const BUDGET: usize = 6 << 30;
    let mut order: Vec<usize> = (0..inputs.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(inputs[i].0.len()));
    let free = std::sync::Mutex::new(BUDGET);
    let freed = std::sync::Condvar::new();
    let packed = parallel_map(&order, |&i| {
        let (bytes, c) = inputs[i];
        let need = if c == Compression::Small { pack_memory(bytes.len()).min(BUDGET) } else { 256 << 20 };
        {
            let mut f = free.lock().unwrap();
            while *f < need {
                f = freed.wait(f).unwrap();
            }
            *f -= need;
        }
        let out = pack_with(bytes, c);
        *free.lock().unwrap() += need;
        freed.notify_all();
        out
    });
    let mut out: Vec<Vec<u8>> = vec![Vec::new(); inputs.len()];
    for (&i, p) in order.iter().zip(packed) {
        out[i] = p;
    }
    out
}

/// The stream a large payload goes to: objects alike compress together, and
/// streams compress in parallel. Textures are grouped by kind (diffuse,
/// normal, mask…): on game data that is as small as all textures in one
/// stream, while splitting a kind lost 20 %.
fn stream_key(class: &str, path: &str) -> String {
    let c = class.to_ascii_lowercase();
    if !c.contains("texture") {
        return c;
    }
    let n = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    let kind = if has(&["nrm", "norm"]) {
        "normal"
    } else if has(&["msk", "mask", "rgb"]) {
        "mask"
    } else if has(&["spc", "spec"]) {
        "specular"
    } else if has(&["emis", "glow"]) {
        "emissive"
    } else if has(&["dif", "albedo", "color", "col"]) || n.ends_with("_d") {
        "diffuse"
    } else {
        "other"
    };
    format!("{c}:{kind}")
}

/// Streams smaller than this stay in the image.
const STREAM_MIN: usize = 1 << 20;

/// The bytes [`pack`] made (or bytes that were stored as they are).
pub fn unpack(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    if bytes.starts_with(ZSTD_MAGIC) {
        let mut out = Vec::new();
        let mut d = zstd::stream::read::Decoder::new(bytes).map_err(|e| Error::Format(format!("compressed delta data: {e}")))?;
        d.window_log_max(27).map_err(|e| Error::Format(format!("compressed delta data: {e}")))?;
        d.read_to_end(&mut out).map_err(|e| Error::Format(format!("compressed delta data: {e}")))?;
        return Ok(out);
    }
    if !bytes.starts_with(XZ_MAGIC) {
        return Ok(bytes.to_vec());
    }
    let mut out = Vec::new();
    xz2::read::XzDecoder::new_multi_decoder(bytes).read_to_end(&mut out).map_err(|e| Error::Format(format!("compressed delta data: {e}")))?;
    Ok(out)
}

/// Compress `bytes` with zstd at its strongest level, using `dict` (the base
/// object's bytes) as a raw-content dictionary: a changed mesh or texture
/// costs about what changed in it.
pub fn pack_from(dict: &[u8], bytes: &[u8]) -> Option<Vec<u8>> {
    pack_from_level(dict, bytes, 19)
}

/// Whether [`pack_from`] is worth it for `bytes`: a quick zstd pass with and
/// without the base object (the strong pass only runs when it is).
fn worth_from(dict: &[u8], bytes: &[u8]) -> bool {
    let alone = zstd::bulk::compress(bytes, 9).map(|z| z.len()).unwrap_or(usize::MAX);
    pack_from_level(dict, bytes, 9).is_some_and(|z| z.len() * 100 < alone * 80)
}

fn pack_from_level(dict: &[u8], bytes: &[u8], level: i32) -> Option<Vec<u8>> {
    use zstd::zstd_safe::CParameter;
    let mut c = zstd::bulk::Compressor::with_dictionary(level, dict).ok()?;
    c.set_parameter(CParameter::WindowLog(27)).ok()?;
    c.set_parameter(CParameter::EnableLongDistanceMatching(true)).ok()?;
    c.compress(bytes).ok()
}

/// The bytes [`pack_from`] made, given the same dictionary.
pub fn unpack_from(dict: &[u8], packed: &[u8], size: usize) -> Result<Vec<u8>> {
    use zstd::zstd_safe::DParameter;
    let mut d = zstd::bulk::Decompressor::with_dictionary(dict).map_err(|e| Error::Format(format!("zstd: {e}")))?;
    d.set_parameter(DParameter::WindowLogMax(27)).map_err(|e| Error::Format(format!("zstd: {e}")))?;
    d.decompress(packed, size).map_err(|e| Error::Format(format!("zstd: {e}")))
}

/// MD5 of an object's bytes, as hex (what a `from` op's dictionary must match).
fn bytes_hash(b: &[u8]) -> String {
    hex(&Md5::digest(b))
}

/// A delta: its JSON header, the mini package image and the shared object
/// files it uses (by hash; loaded or embedded).
#[derive(Clone, Debug, Default)]
pub struct Delta {
    pub header: Value,
    pub image: Vec<u8>,
    pub shared: BTreeMap<String, Vec<u8>>,
    /// Payloads compressed against the base object (by mini export): see [`pack_from`].
    pub from: BTreeMap<usize, Vec<u8>>,
    /// Large payloads grouped by kind ([`stream_key`]), each compressed on its
    /// own; an op's `stream: [s, offset, len]` says where its payload is.
    pub streams: Vec<Vec<u8>>,
    /// How the file is compressed when written (not stored: readers detect it).
    pub compression: Compression,
}

/// How [`create_with`] builds a delta.
#[derive(Clone, Debug, Default)]
pub struct CreateOptions {
    /// Move payloads of at least this many bytes into shared object files.
    pub share_min: Option<usize>,
    /// What changes, not how small it can be stored: no payloads compressed
    /// against their base object (previews; `patch` ops are still made).
    pub fast: bool,
    /// Share only these objects (by hash); the others stay in the delta's own
    /// image, where they compress together with the objects like them.
    pub share_only: Option<std::sync::Arc<HashSet<String>>>,
    /// How the delta is compressed when written; [`Compression::Fast`] also
    /// compresses payloads against their base object at a lower level.
    pub compression: Compression,
}

/// `f` over `items` on every core, results in order.
pub fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(items.len().max(1));
    if threads <= 1 {
        return items.iter().map(&f).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<Option<R>> = (0..items.len()).map(|_| None).collect();
    let slots = std::sync::Mutex::new(&mut out);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(item) = items.get(i) else { break };
                let r = f(item);
                slots.lock().unwrap()[i] = Some(r);
            });
        }
    });
    out.into_iter().map(|r| r.expect("every item mapped")).collect()
}

/// One operation of the header, as the launcher and the Studio see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Op {
    /// `add`, `set` or `delete`.
    pub op: String,
    pub path: String,
    pub class: String,
    /// Canonical hash of the base object a `set` replaces (hex), when known.
    pub base: Option<String>,
    /// Export of the mini package carrying the payload (`add` / `set`).
    pub export: Option<usize>,
    /// Hash of the shared object file holding the payload instead.
    pub shared: Option<String>,
    /// A `patch` op: `{remove: [[name, index]…], after: [[name, index] | null…]}`
    /// (the mini export holds only the changed properties).
    pub patch: Option<Value>,
    /// The payload is compressed against the base object: `{hash, size}` (the
    /// MD5 of the base object's bytes, the payload's size).
    pub from: Option<Value>,
    /// The payload is in a stream: `(stream, offset, length)`.
    pub stream: Option<(usize, usize, usize)>,
}

impl Delta {
    pub fn kind(&self) -> &str {
        self.header["kind"].as_str().unwrap_or("")
    }

    /// Game-relative path of the file the delta applies to.
    pub fn target(&self) -> &str {
        self.header["target"].as_str().unwrap_or("")
    }

    pub fn ops(&self) -> Vec<Op> {
        self.header["ops"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|o| Op {
                        op: o["op"].as_str().unwrap_or("").to_string(),
                        path: o["path"].as_str().unwrap_or("").to_string(),
                        class: o["class"].as_str().unwrap_or("").to_string(),
                        base: o["base"].as_str().map(str::to_string),
                        export: o["export"].as_u64().map(|v| v as usize),
                        shared: o["shared"]["hash"].as_str().map(str::to_string),
                        patch: o.get("patch").cloned(),
                        from: o.get("from").cloned(),
                        stream: o["stream"].as_array().and_then(|a| Some((a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize, a.get(2)?.as_u64()? as usize))),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn layout(&self) -> Layout {
        Layout { editor_data: self.header["layout"]["editor_data"].as_bool().unwrap_or(true) }
    }

    /// Classes the delta adds that the applier's class database may not know.
    pub fn added_classes(&self) -> Vec<String> {
        self.ops().into_iter().filter(|o| o.op == "add" && o.class.eq_ignore_ascii_case("Class")).map(|o| o.path.rsplit('.').next().unwrap_or("").to_string()).collect()
    }

    /// Hashes of the shared object files the delta's ops use.
    pub fn shared_hashes(&self) -> Vec<String> {
        let mut out: Vec<String> = self.ops().into_iter().filter_map(|o| o.shared).collect();
        out.sort();
        out.dedup();
        out
    }

    /// The delta with its shared object files embedded: one self-contained file.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode(true)
    }

    /// The delta without its shared object files (they are written next to it,
    /// see [`Delta::write_shared`]).
    pub fn to_bytes_external(&self) -> Vec<u8> {
        self.encode(false)
    }

    /// What [`Delta::assemble`] compresses: the image, then every stream (and,
    /// when `embed`, every shared object file the ops use).
    pub fn parts(&self, embed: bool) -> Vec<&[u8]> {
        let mut parts: Vec<&[u8]> = vec![&self.image];
        parts.extend(self.streams.iter().map(|s| s.as_slice()));
        if embed {
            parts.extend(self.shared_hashes().iter().filter_map(|h| self.shared.get(h)).map(|b| b.as_slice()));
        }
        parts
    }

    fn encode(&self, embed: bool) -> Vec<u8> {
        let parts: Vec<(&[u8], Compression)> = self.parts(embed).into_iter().map(|b| (b, self.compression)).collect();
        self.assemble(pack_all_with(&parts), embed)
    }

    /// The file, from [`Delta::parts`] compressed with [`pack`] (in the same order).
    pub fn assemble(&self, packed: Vec<Vec<u8>>, embed: bool) -> Vec<u8> {
        let mut packed = packed.into_iter();
        let hashes = self.shared_hashes();
        let image = packed.next().unwrap_or_default();
        let streams: Vec<Vec<u8>> = (0..self.streams.len()).map(|_| packed.next().unwrap_or_default()).collect();
        let compressed = image.len() != self.image.len() || image.starts_with(XZ_MAGIC);
        let version = if hashes.is_empty() && !compressed && self.from.is_empty() && self.streams.is_empty() { 1 } else { VERSION };
        let mut header = self.header.clone();
        header["version"] = json!(version);
        let json = serde_json::to_vec(&header).unwrap_or_default();
        let mut w = Writer::new();
        w.bytes(MAGIC);
        w.u32(version);
        w.u32(json.len() as u32);
        w.bytes(&json);
        w.u32(image.len() as u32);
        w.bytes(&image);
        if version >= 2 {
            let embedded: Vec<&String> = if embed { hashes.iter().filter(|h| self.shared.contains_key(*h)).collect() } else { Vec::new() };
            w.u32(embedded.len() as u32);
            for h in embedded {
                let bytes = packed.next().unwrap_or_default();
                w.u32(h.len() as u32);
                w.bytes(h.as_bytes());
                w.u32(bytes.len() as u32);
                w.bytes(&bytes);
            }
            w.u32(self.from.len() as u32);
            for (k, bytes) in &self.from {
                w.u32(*k as u32);
                w.u32(bytes.len() as u32);
                w.bytes(bytes);
            }
            w.u32(streams.len() as u32);
            for bytes in &streams {
                w.u32(bytes.len() as u32);
                w.bytes(bytes);
            }
        }
        w.into_inner()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Delta> {
        let mut r = Reader::new(bytes);
        if r.take(4)? != MAGIC {
            return Err(Error::Format("not a Tempest delta (bad magic)".into()));
        }
        let version = r.u32()?;
        if version == 0 || version > VERSION {
            return Err(Error::Format(format!("delta version {version} is not supported (this build reads up to {VERSION}); update Tempest")));
        }
        let n = r.u32()? as usize;
        let header: Value = serde_json::from_slice(r.take(n)?).map_err(|e| Error::Format(format!("delta header: {e}")))?;
        let n = r.u32()? as usize;
        let image = unpack(r.take(n)?)?;
        let mut shared = BTreeMap::new();
        if version >= 2 {
            let count = r.u32()?;
            for _ in 0..count {
                let n = r.u32()? as usize;
                let hash = String::from_utf8_lossy(r.take(n)?).to_string();
                let n = r.u32()? as usize;
                let bytes = unpack(r.take(n)?)?;
                shared.insert(hash, bytes);
            }
        }
        let mut from = BTreeMap::new();
        if version >= 2 && r.remaining() >= 4 {
            let count = r.u32()?;
            for _ in 0..count {
                let k = r.u32()? as usize;
                let n = r.u32()? as usize;
                from.insert(k, r.take(n)?.to_vec());
            }
        }
        let mut packed_streams: Vec<&[u8]> = Vec::new();
        if version >= 2 && r.remaining() >= 4 {
            let count = r.u32()?;
            for _ in 0..count {
                let n = r.u32()? as usize;
                packed_streams.push(r.take(n)?);
            }
        }
        let streams = parallel_map(&packed_streams, |b| unpack(b)).into_iter().collect::<Result<Vec<_>>>()?;
        Ok(Delta { header, image, shared, from, streams, compression: Compression::Small })
    }

    /// Read a delta and the shared object files it uses: embedded ones, else
    /// `shared/<hash>.tobj` in the delta's folder or any folder above it (a
    /// mod's `deltas/…` tree keeps them at the mod root).
    pub fn read(path: impl AsRef<Path>) -> Result<Delta> {
        let path = path.as_ref();
        let mut d = Delta::from_bytes(&std::fs::read(path)?)?;
        let missing: Vec<String> = d.shared_hashes().into_iter().filter(|h| !d.shared.contains_key(h)).collect();
        for hash in missing {
            let file = shared_file_for(path, &hash).ok_or_else(|| Error::Format(format!("{}: shared object {hash}.{SHARED_EXT} not found (expected in a shared/ folder of the mod)", path.display())))?;
            let bytes = unpack(&std::fs::read(&file)?)?;
            if hex(&Sha256::digest(&bytes)) != hash {
                return Err(Error::Format(format!("{}: {} does not match its hash", path.display(), file.display())));
            }
            d.shared.insert(hash, bytes);
        }
        Ok(d)
    }

    /// Write the delta as one self-contained file.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        std::fs::write(path, self.to_bytes())?;
        Ok(())
    }

    /// Write the delta to `path` and its shared object files into `store`
    /// (`<store>/<hash>.tobj`, skipped when already there): how a mod ships
    /// them, once for every delta that uses them.
    pub fn write_shared(&self, path: impl AsRef<Path>, store: impl AsRef<Path>) -> Result<()> {
        let store = store.as_ref();
        let hashes = self.shared_hashes();
        if !hashes.is_empty() {
            std::fs::create_dir_all(store)?;
        }
        let todo: Vec<(PathBuf, &Vec<u8>)> = hashes.iter().filter_map(|h| Some((store.join(format!("{h}.{SHARED_EXT}")), self.shared.get(h)?))).filter(|(f, _)| !f.exists()).collect();
        for (r, (file, _)) in parallel_map(&todo, |(_, b)| pack_with(b, self.compression)).into_iter().zip(&todo) {
            std::fs::write(file, r)?;
        }
        std::fs::write(path, self.to_bytes_external())?;
        Ok(())
    }
}

/// `shared/<hash>.tobj` in the folder of `delta` or the nearest folder above it.
fn shared_file_for(delta: &Path, hash: &str) -> Option<PathBuf> {
    let name = format!("{hash}.{SHARED_EXT}");
    let mut dir = delta.parent();
    while let Some(d) = dir {
        let f = d.join("shared").join(&name);
        if f.is_file() {
            return Some(f);
        }
        dir = d.parent();
    }
    None
}

// ----------------------------------------------------------------------------
// Canonical form
// ----------------------------------------------------------------------------

/// Table-independent description of a reference: `I|class package|class|path`
/// for an import, `E|class|path` for an export, empty for `None`.
fn describe(pkg: &Package, idx: PackageIndex) -> String {
    if idx.is_null() {
        return String::new();
    }
    let s = if let Some(i) = idx.import_index() {
        let imp = &pkg.imports[i];
        format!("I|{}|{}|{}", pkg.fname_to_string(imp.class_package), pkg.fname_to_string(imp.class_name), pkg.object_path(idx))
    } else {
        format!("E|{}|{}", pkg.class_name_of(idx), pkg.object_path(idx))
    };
    s.to_ascii_lowercase()
}

/// The cooker's zero-size, class-less placeholder (also what `obj delete` leaves).
fn is_placeholder(pkg: &Package, i: usize) -> bool {
    let e = &pkg.exports[i];
    e.class.is_null() && pkg.export_data.get(i).is_none_or(|d| d.is_empty()) && pkg.fname_to_string(e.object_name).eq_ignore_ascii_case("None")
}

/// Renames every name and reference of an object to the order of its first
/// appearance, recording what they were; the written result then depends
/// only on the object.
struct Canon<'a> {
    pkg: &'a Package,
    names: Vec<String>,
    refs: Vec<String>,
    name_map: HashMap<u32, u32>,
    ref_map: HashMap<PackageIndex, PackageIndex>,
}

impl<'a> Canon<'a> {
    fn new(pkg: &'a Package) -> Canon<'a> {
        let none = none_name(pkg);
        Canon { pkg, names: vec!["None".to_string()], refs: Vec::new(), name_map: HashMap::from([(none.index, 0)]), ref_map: HashMap::new() }
    }
}

impl Visitor for Canon<'_> {
    fn name(&mut self, n: &mut FName) {
        let next = self.names.len() as u32;
        let idx = *self.name_map.entry(n.index).or_insert_with(|| {
            self.names.push(self.pkg.name_str(n.index).to_ascii_lowercase());
            next
        });
        n.index = idx;
    }
    fn index(&mut self, i: &mut PackageIndex) {
        if i.is_null() {
            return;
        }
        let next = PackageIndex::export(self.refs.len());
        let m = *self.ref_map.entry(*i).or_insert_with(|| {
            self.refs.push(describe(self.pkg, *i));
            next
        });
        *i = m;
    }
}

/// Canonical hash of export `i`: `None` for a placeholder.
pub fn canonical(db: &ClassDb, pkg: &Package, layout: Layout, i: usize) -> Option<[u8; 16]> {
    if is_placeholder(pkg, i) {
        return None;
    }
    let e = &pkg.exports[i];
    let mut h = Md5::new();
    // A null class means `UClass`; the compiler writes the same thing as an
    // explicit `Core.Class` import.
    let class_desc = describe(pkg, e.class);
    h.update(if e.class.is_null() || class_desc == "i|core|class|core.class" { "class" } else { class_desc.as_str() });
    h.update(b"\0");
    for r in [e.super_, e.archetype] {
        h.update(describe(pkg, r));
        h.update(b"\0");
    }
    h.update(e.object_flags.to_le_bytes());
    h.update(e.export_flags.to_le_bytes());
    h.update(e.package_flags.to_le_bytes());
    for c in &e.generation_net_object_count {
        h.update(c.to_le_bytes());
    }
    let data = pkg.export_data.get(i).map(|d| d.as_slice()).unwrap_or(&[]);
    let mut p = parse_export(db, pkg, layout, i, data);
    if let Parsed::Raw { data, .. } = &p {
        h.update(b"raw\0");
        h.update(data);
        return Some(h.finalize().into());
    }
    let mut c = Canon::new(pkg);
    let none = none_name(pkg);
    let problems = visit_parsed(&mut p, Some(none.index), &mut c);
    let bytes = write_parsed(&p, layout, FName::new(0));
    for n in &c.names {
        h.update(n.as_bytes());
        h.update(b"\0");
    }
    h.update(b"\x01");
    for r in &c.refs {
        h.update(r.as_bytes());
        h.update(b"\0");
    }
    h.update(b"\x02");
    for p in &problems {
        h.update(p.as_bytes());
    }
    h.update(&bytes);
    if let Ok(want) = std::env::var("TDELTA_DEBUG") {
        let path = pkg.object_path(PackageIndex::export(i));
        if path.eq_ignore_ascii_case(&want) {
            eprintln!("[tdelta] {path}: class={} super={} arch={} flags={:#x}/{:#x}/{:#x} netcounts={:?}", describe(pkg, e.class), describe(pkg, e.super_), describe(pkg, e.archetype), e.object_flags, e.export_flags, e.package_flags, e.generation_net_object_count);
            eprintln!("[tdelta]   names={:?}", c.names);
            eprintln!("[tdelta]   refs={:?}", c.refs);
            eprintln!("[tdelta]   problems={:?} payload={} bytes md5={}", problems, bytes.len(), hex(&Md5::digest(&bytes)));
        }
    }
    Some(h.finalize().into())
}

fn hex(h: &[u8]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// Lower-case path → export, for every export that is not a placeholder.
fn path_index(pkg: &Package) -> HashMap<String, usize> {
    let mut m = HashMap::with_capacity(pkg.exports.len());
    for i in 0..pkg.exports.len() {
        if !is_placeholder(pkg, i) {
            m.insert(pkg.object_path(PackageIndex::export(i)).to_ascii_lowercase(), i);
        }
    }
    m
}

/// The edited package's tables extend the base's (the editor appends; a
/// deleted export becomes a placeholder), so equal payload bytes mean equal
/// objects. A rebuilt package (script recompile) fails this and every
/// export is compared canonically.
fn tables_compatible(base: &Package, edited: &Package) -> bool {
    if edited.names.len() < base.names.len() || edited.imports.len() < base.imports.len() || edited.exports.len() < base.exports.len() {
        return false;
    }
    if base.names.iter().zip(&edited.names).any(|(a, b)| a.name != b.name) {
        return false;
    }
    if base.imports.iter().zip(&edited.imports).any(|(a, b)| a != b) {
        return false;
    }
    for (i, (a, b)) in base.exports.iter().zip(&edited.exports).enumerate() {
        if is_placeholder(edited, i) {
            continue;
        }
        if a.object_name != b.object_name || a.outer != b.outer {
            return false;
        }
    }
    true
}

// ----------------------------------------------------------------------------
// Digest (the Studio's record of a package before it was first saved)
// ----------------------------------------------------------------------------

/// Canonical hash and class of every object of a package: enough to create
/// a delta later without the original file.
pub fn digest(db: &ClassDb, pkg: &Package) -> Value {
    let layout = package_layout(pkg);
    let mut exports = serde_json::Map::new();
    for i in 0..pkg.exports.len() {
        if let Some(h) = canonical(db, pkg, layout, i) {
            let idx = PackageIndex::export(i);
            exports.insert(pkg.object_path(idx).to_ascii_lowercase(), json!([pkg.class_name_of(idx), hex(&h)]));
        }
    }
    json!({ "format": "tdelta-digest", "version": VERSION, "package": pkg.package_name(), "exports": exports })
}

/// What a delta is created against: the base package itself, or its digest.
pub enum Base<'a> {
    Package(&'a Package),
    Digest(&'a Value),
}

// ----------------------------------------------------------------------------
// Create
// ----------------------------------------------------------------------------

enum Change {
    Add(usize),
    /// Edited export, base hash, base export (when the base is a package).
    Set(usize, String, Option<usize>),
}

/// A property's key in an object: its name (any case) and array index.
fn prop_key(pkg: &Package, p: &crate::value::Property) -> (String, i32) {
    (pkg.name_str(p.name.index).to_ascii_lowercase(), p.array_index)
}

/// Table-independent bytes of a parsed object (names and references as text).
fn canon_bytes(pkg: &Package, layout: Layout, mut p: Parsed) -> Vec<u8> {
    let mut c = Canon::new(pkg);
    let none = none_name(pkg);
    let problems = visit_parsed(&mut p, Some(none.index), &mut c);
    let mut out = Vec::new();
    for n in &c.names {
        out.extend_from_slice(n.as_bytes());
        out.push(0);
    }
    out.push(1);
    for r in &c.refs {
        out.extend_from_slice(r.as_bytes());
        out.push(0);
    }
    out.push(2);
    for pr in &problems {
        out.extend_from_slice(pr.as_bytes());
    }
    out.extend_from_slice(&write_parsed(&p, layout, FName::new(0)));
    out
}

/// An object holding one property and nothing else (to compare properties).
fn just(o: &crate::object::Object, p: &crate::value::Property) -> Parsed {
    Parsed::Object(crate::object::Object {
        class: o.class,
        class_name: o.class_name.clone(),
        native_prefix: Vec::new(),
        state_frame: None,
        component: None,
        net_index: 0,
        properties: vec![p.clone()],
        native: crate::object::Native::Raw(Vec::new()),
        is_class: false,
    })
}

/// When `edited` export `i` differs from `base` export `j` only in tagged
/// properties: the changed or added properties (edited's, in its order), the
/// keys of the removed ones and, per changed property, the key of the
/// property before it in the edited object (where an added one goes).
#[allow(clippy::type_complexity)]
fn property_patch(db: &ClassDb, base: &Package, j: usize, edited: &Package, i: usize, eo: &crate::object::Object, layout: Layout) -> Option<(Vec<crate::value::Property>, Vec<Value>, Vec<Value>)> {
    let (a, b) = (&base.exports[j], &edited.exports[i]);
    if describe(base, a.class) != describe(edited, b.class) || describe(base, a.super_) != describe(edited, b.super_) || describe(base, a.archetype) != describe(edited, b.archetype) || a.object_flags != b.object_flags || a.export_flags != b.export_flags || a.package_flags != b.package_flags || a.generation_net_object_count != b.generation_net_object_count {
        return None;
    }
    let bl = package_layout(base);
    let bdata = base.export_data.get(j).map(|d| d.as_slice()).unwrap_or(&[]);
    let Parsed::Object(bo) = parse_export(db, base, bl, j, bdata) else { return None };
    if eo.is_class || bo.is_class || eo.class.is_none() || eo.has_raw_values() || bo.has_raw_values() {
        return None;
    }
    // Everything but the properties must be the same.
    let rest = |pkg: &Package, o: &crate::object::Object, l: Layout| {
        let mut r = o.clone();
        r.properties.clear();
        canon_bytes(pkg, l, Parsed::Object(r))
    };
    if rest(base, &bo, bl) != rest(edited, eo, layout) {
        return None;
    }
    let mut base_props: HashMap<(String, i32), Vec<u8>> = HashMap::new();
    for p in &bo.properties {
        if base_props.insert(prop_key(base, p), canon_bytes(base, bl, just(&bo, p))).is_some() {
            return None;
        }
    }
    let mut seen: HashSet<(String, i32)> = HashSet::new();
    let (mut changed, mut after) = (Vec::new(), Vec::new());
    let mut prev: Option<(String, i32)> = None;
    for p in &eo.properties {
        let k = prop_key(edited, p);
        if !seen.insert(k.clone()) {
            return None;
        }
        if base_props.get(&k) != Some(&canon_bytes(edited, layout, just(eo, p))) {
            changed.push(p.clone());
            after.push(prev.as_ref().map(|(n, x)| json!([n, x])).unwrap_or(Value::Null));
        }
        prev = Some(k);
    }
    let remove: Vec<Value> = bo.properties.iter().map(|p| prop_key(base, p)).filter(|k| !seen.contains(k)).map(|(n, x)| json!([n, x])).collect();
    if changed.is_empty() && remove.is_empty() {
        return None;
    }
    Some((changed, remove, after))
}

/// Compare `edited` with its base and build the delta for `target` (the
/// game-relative path of the file). Returns the delta and warnings about
/// payloads whose references could not be remapped.
pub fn create(db: &ClassDb, base: Base, edited: &Package, target: &str) -> Result<(Delta, Vec<String>)> {
    create_with(db, base, edited, target, CreateOptions::default())
}

/// The summary of a shared object file: what parsing needs (versions, flags),
/// nothing that names the package it came from, so the same object gives the
/// same bytes wherever it was cooked.
fn shared_summary(from: &tempest_upk::Summary) -> tempest_upk::Summary {
    let mut s = from.clone();
    s.folder_name = "None".into();
    s.guid = Default::default();
    // A checksum of the package the object was cooked into.
    s.package_source = 0;
    s.compressed_chunks.clear();
    s.compression_flags = 0;
    s.texture_allocations.clear();
    s.additional_packages_to_cook.clear();
    s.generations = vec![Default::default()];
    s
}

/// [`create`] with options (shared object files).
pub fn create_with(db: &ClassDb, base: Base, edited: &Package, target: &str, opts: CreateOptions) -> Result<(Delta, Vec<String>)> {
    let layout = package_layout(edited);
    let mut shared: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut from: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    // Ops whose payload may be stored against the base object: (op, mini export, base export).
    let mut candidates: Vec<(usize, usize, usize)> = Vec::new();
    let mut warnings = Vec::new();
    let edited_paths = path_index(edited);

    let base_pkg = match &base {
        Base::Package(b) => Some(*b),
        Base::Digest(_) => None,
    };
    // What changed, by path.
    let mut changes: Vec<(String, Change)> = Vec::new();
    let mut deletes: Vec<(String, String)> = Vec::new();
    match base {
        Base::Package(base) => {
            let base_layout = package_layout(base);
            let base_paths = path_index(base);
            // Equal bytes only mean equal objects while every export they can
            // point at still means the same thing: a deleted export (now a
            // placeholder) changes what an untouched reference to it means.
            let deleted_any = base_paths.keys().any(|p| !edited_paths.contains_key(p));
            let fast = !deleted_any && tables_compatible(base, edited);
            for (path, &i) in &edited_paths {
                match base_paths.get(path) {
                    None => changes.push((path.clone(), Change::Add(i))),
                    Some(&j) => {
                        let (a, b) = (&base.exports[j], &edited.exports[i]);
                        let same_entry = a.class == b.class && a.super_ == b.super_ && a.archetype == b.archetype && a.object_flags == b.object_flags && a.export_flags == b.export_flags;
                        if fast && same_entry && base.export_data.get(j) == edited.export_data.get(i) {
                            continue;
                        }
                        let hb = canonical(db, base, base_layout, j);
                        let he = canonical(db, edited, layout, i);
                        if hb != he {
                            changes.push((path.clone(), Change::Set(i, hb.map(|h| hex(&h)).unwrap_or_default(), Some(j))));
                        }
                    }
                }
            }
            for (path, &j) in &base_paths {
                if !edited_paths.contains_key(path) {
                    deletes.push((path.clone(), base.class_name_of(PackageIndex::export(j))));
                }
            }
        }
        Base::Digest(d) => {
            let empty = serde_json::Map::new();
            let exports = d["exports"].as_object().unwrap_or(&empty);
            for (path, &i) in &edited_paths {
                match exports.get(path) {
                    None => changes.push((path.clone(), Change::Add(i))),
                    Some(entry) => {
                        let hb = entry[1].as_str().unwrap_or("");
                        let he = canonical(db, edited, layout, i).map(|h| hex(&h)).unwrap_or_default();
                        if hb != he {
                            changes.push((path.clone(), Change::Set(i, hb.to_string(), None)));
                        }
                    }
                }
            }
            for (path, entry) in exports {
                if !edited_paths.contains_key(path) {
                    deletes.push((path.clone(), entry[0].as_str().unwrap_or("").to_string()));
                }
            }
        }
    }
    // Outers before their children, then a stable order.
    changes.sort_by(|a, b| (a.0.matches('.').count(), &a.0).cmp(&(b.0.matches('.').count(), &b.0)));
    deletes.sort();

    // The mini package.
    let mut mini = Package::default();
    mini.summary = edited.summary.clone();
    mini.summary.compressed_chunks.clear();
    mini.summary.compression_flags = 0;
    mini.summary.texture_allocations.clear();
    mini.summary.additional_packages_to_cook.clear();
    mini.summary.generations = vec![Default::default()];
    mini.intern_name("None");
    let op_of: HashMap<usize, usize> = changes.iter().enumerate().map(|(k, (_, c))| (match c { Change::Add(i) | Change::Set(i, _, _) => *i }, k)).collect();
    // Reserve the mini exports so references between them resolve to their index.
    for (_, c) in &changes {
        let i = match c { Change::Add(i) | Change::Set(i, _, _) => *i };
        let e = &edited.exports[i];
        let name = edited.object_name(PackageIndex::export(i));
        let export = ObjectExport { object_name: mini.intern_fname(&name), object_flags: e.object_flags, export_flags: e.export_flags, generation_net_object_count: e.generation_net_object_count.clone(), package_guid: e.package_guid, package_flags: e.package_flags, ..Default::default() };
        mini.add_export(export, Vec::new());
    }

    let mut refs: Vec<Value> = Vec::new();
    let mut ref_memo: HashMap<PackageIndex, PackageIndex> = HashMap::new();
    // A reference of the edited package in mini-package coordinates: one of
    // the delta's exports, or an import describing where to find it.
    fn map_ref(edited: &Package, mini: &mut Package, idx: PackageIndex, op_of: &HashMap<usize, usize>, refs: &mut Vec<Value>, memo: &mut HashMap<PackageIndex, PackageIndex>) -> PackageIndex {
        if idx.is_null() {
            return PackageIndex::NONE;
        }
        if let Some(&m) = memo.get(&idx) {
            return m;
        }
        if let Some(i) = idx.export_index() {
            if let Some(&k) = op_of.get(&i) {
                let m = PackageIndex::export(k);
                memo.insert(idx, m);
                return m;
            }
        }
        let outer = map_ref(edited, mini, edited.object_outer(idx), op_of, refs, memo);
        let name = edited.object_name(idx);
        let (class_package, class_name, desc) = if let Some(i) = idx.import_index() {
            let imp = &edited.imports[i];
            (edited.fname_to_string(imp.class_package), edited.fname_to_string(imp.class_name), json!({ "k": "import" }))
        } else {
            let e = &edited.exports[idx.export_index().unwrap()];
            let class_name = edited.class_name_of(idx);
            let class_package = if e.class.is_null() { "Core".to_string() } else { edited.outermost_name(e.class) };
            (class_package, class_name.clone(), json!({ "k": "export", "path": edited.object_path(idx), "class": class_name }))
        };
        let before = mini.imports.len();
        let m = mini.add_import(&class_package, &class_name, outer, &name);
        if mini.imports.len() > before {
            refs.push(desc);
        }
        memo.insert(idx, m);
        m
    }

    let none = none_name(edited);
    let mut ops: Vec<Value> = Vec::new();
    let mut unknown_classes: HashSet<String> = HashSet::new();
    for (k, (path, c)) in changes.iter().enumerate() {
        let (i, op, base_hash, base_j) = match c {
            Change::Add(i) => (*i, "add", None, None),
            Change::Set(i, h, j) => (*i, "set", Some(h.clone()), *j),
        };
        let e = edited.exports[i].clone();
        let table: Vec<PackageIndex> = [e.class, e.super_, e.outer, e.archetype].into_iter().map(|r| map_ref(edited, &mut mini, r, &op_of, &mut refs, &mut ref_memo)).collect();
        {
            let me = &mut mini.exports[k];
            me.class = table[0];
            me.super_ = table[1];
            me.outer = table[2];
            me.archetype = table[3];
        }
        let data = edited.export_data.get(i).map(|d| d.as_slice()).unwrap_or(&[]);
        let mut p = parse_export(db, edited, layout, i, data);
        // An object whose class the database does not know (or with values it
        // could not type) keeps raw bytes, and references inside raw bytes
        // cannot be remapped: the delta would carry this package's indices.
        if let Parsed::Object(o) = &p {
            if o.class.is_none() && !o.properties.is_empty() {
                let class = edited.class_name_of(PackageIndex::export(i));
                if !unknown_classes.contains(&class) {
                    unknown_classes.insert(class.clone());
                    warnings.push(format!("class {class} is not in the class database (wrong -I folder?): references inside its objects are copied as they are"));
                }
            } else if o.has_raw_values() {
                warnings.push(format!("{}: some values could not be typed; references inside them are copied as they are", edited.object_path(PackageIndex::export(i))));
            }
        }
        // Only properties changed: ship those (a `patch`, merged into the
        // object as it is when the delta is applied) instead of the object.
        let mut patch: Option<Value> = None;
        if let (Some(j), Some(bp), Parsed::Object(eo)) = (base_j, base_pkg, &p) {
            if let Some((props, remove, after)) = property_patch(db, bp, j, edited, i, eo, layout) {
                let o = crate::object::Object { class: eo.class, class_name: eo.class_name.clone(), native_prefix: eo.native_prefix.clone(), state_frame: eo.state_frame.clone(), component: eo.component.clone(), net_index: eo.net_index, properties: props, native: crate::object::Native::Raw(Vec::new()), is_class: false };
                p = Parsed::Object(o);
                patch = Some(json!({ "remove": remove, "after": after }));
            }
        }
        let op = if patch.is_some() { "patch" } else { op };
        struct Remap<'s> {
            edited: &'s Package,
            mini: &'s mut Package,
            op_of: &'s HashMap<usize, usize>,
            refs: &'s mut Vec<Value>,
            memo: &'s mut HashMap<PackageIndex, PackageIndex>,
            names: HashMap<u32, u32>,
        }
        impl Visitor for Remap<'_> {
            fn name(&mut self, n: &mut FName) {
                let idx = *self.names.entry(n.index).or_insert_with(|| self.mini.intern_name(self.edited.name_str(n.index)));
                n.index = idx;
            }
            fn index(&mut self, i: &mut PackageIndex) {
                *i = map_ref(self.edited, self.mini, *i, self.op_of, self.refs, self.memo);
            }
        }
        let mut remap = Remap { edited, mini: &mut mini, op_of: &op_of, refs: &mut refs, memo: &mut ref_memo, names: HashMap::new() };
        for problem in visit_parsed(&mut p, Some(none.index), &mut remap) {
            warnings.push(format!("{}: {problem}", edited.object_path(PackageIndex::export(i))));
        }
        let mini_none = none_name(&mini);
        let bytes = write_parsed(&p, layout, mini_none);
        let class = edited.class_name_of(PackageIndex::export(i));
        if let Some(patch) = patch {
            mini.set_export_data(k, bytes);
            ops.push(json!({ "op": op, "path": path, "class": class, "base": base_hash, "export": k, "patch": patch }));
            continue;
        }
        if opts.share_min.is_some_and(|min| bytes.len() >= min) {
            // A shared object file: a package of this one export whose names
            // and imports come from the object alone (references to anything
            // else, the delta's other objects included, resolve by path).
            let mut tiny = Package::default();
            tiny.summary = shared_summary(&edited.summary);
            tiny.intern_name("None");
            let name = edited.object_name(PackageIndex::export(i));
            let export = ObjectExport { object_name: tiny.intern_fname(&name), object_flags: e.object_flags, export_flags: e.export_flags, ..Default::default() };
            tiny.add_export(export, Vec::new());
            let self_of: HashMap<usize, usize> = HashMap::from([(i, 0)]);
            let mut trefs: Vec<Value> = Vec::new();
            let mut tmemo: HashMap<PackageIndex, PackageIndex> = HashMap::new();
            let table: Vec<PackageIndex> = [e.class, e.super_, e.outer, e.archetype].into_iter().map(|r| map_ref(edited, &mut tiny, r, &self_of, &mut trefs, &mut tmemo)).collect();
            {
                let te = &mut tiny.exports[0];
                te.class = table[0];
                te.super_ = table[1];
                te.outer = table[2];
                te.archetype = table[3];
            }
            let mut tp = parse_export(db, edited, layout, i, data);
            let mut tremap = Remap { edited, mini: &mut tiny, op_of: &self_of, refs: &mut trefs, memo: &mut tmemo, names: HashMap::new() };
            let _ = visit_parsed(&mut tp, Some(none.index), &mut tremap);
            let tiny_none = none_name(&tiny);
            let tbytes = write_parsed(&tp, layout, tiny_none);
            tiny.set_export_data(0, tbytes);
            let image = tiny.to_bytes(&WriteOptions { compress: false, obscure: false });
            let hash = hex(&Sha256::digest(&image));
            if opts.share_only.as_ref().is_none_or(|only| only.contains(&hash)) {
                shared.insert(hash.clone(), image);
                ops.push(json!({ "op": op, "path": path, "class": class, "base": base_hash, "export": k, "shared": { "hash": hash, "refs": trefs } }));
                continue;
            }
        }
        {
            // A large payload compressed against the base object when that is
            // clearly smaller than compressing it alone (a texture or mesh
            // changed in part, or replaced by a similar one).
            // Decided below, every candidate at once on all cores.
            if let (Some(j), Some(_), false) = (base_j, base_pkg, opts.fast) {
                if bytes.len() >= 4096 && base_pkg.and_then(|bp| bp.export_data.get(j)).is_some_and(|d| d.len() >= 4096) {
                    candidates.push((ops.len(), k, j));
                }
            }
            mini.set_export_data(k, bytes);
            ops.push(json!({ "op": op, "path": path, "class": class, "base": base_hash, "export": k }));
        }
    }
    if let Some(bp) = base_pkg {
        let decided = parallel_map(&candidates, |&(_, k, j)| {
            let (dict, bytes) = (&bp.export_data[j], &mini.export_data[k]);
            if worth_from(dict, bytes) { pack_from_level(dict, bytes, if opts.compression == Compression::Fast { 12 } else { 19 }) } else { None }
        });
        for (&(o, k, j), z) in candidates.iter().zip(decided) {
            let Some(z) = z else { continue };
            ops[o]["from"] = json!({ "hash": bytes_hash(&bp.export_data[j]), "size": mini.export_data[k].len() });
            from.insert(k, z);
            mini.set_export_data(k, Vec::new());
        }
    }
    // Large payloads into streams of their kind (compressed in parallel).
    let mut streams: Vec<Vec<u8>> = Vec::new();
    {
        let mut groups: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
        for (o, op) in ops.iter().enumerate() {
            let Some(k) = op["export"].as_u64().map(|v| v as usize) else { continue };
            if op.get("patch").is_some() || op.get("shared").is_some() || op.get("from").is_some() || mini.export_data[k].len() < 4096 {
                continue;
            }
            groups.entry(stream_key(op["class"].as_str().unwrap_or(""), op["path"].as_str().unwrap_or(""))).or_default().push((o, k));
        }
        for members in groups.into_values() {
            if members.iter().map(|&(_, k)| mini.export_data[k].len()).sum::<usize>() < STREAM_MIN {
                continue;
            }
            let s = streams.len();
            let mut stream = Vec::new();
            for (o, k) in members {
                let data = std::mem::take(&mut mini.export_data[k]);
                mini.set_export_data(k, Vec::new());
                ops[o]["stream"] = json!([s, stream.len(), data.len()]);
                stream.extend_from_slice(&data);
            }
            streams.push(stream);
        }
    }
    for (path, class) in &deletes {
        ops.push(json!({ "op": "delete", "path": path, "class": class }));
    }

    let image = mini.to_bytes(&WriteOptions { compress: false, obscure: false });
    let header = json!({
        "format": "tdelta",
        "version": VERSION,
        "kind": "upk",
        "target": target,
        "package": Path::new(target).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        "layout": { "editor_data": layout.editor_data },
        "refs": refs,
        "ops": ops,
    });
    Ok((Delta { header, image, shared, from, streams, compression: opts.compression }, warnings))
}

// ----------------------------------------------------------------------------
// Apply
// ----------------------------------------------------------------------------

/// Which operations an [`apply`] pass handles. A delta adding classes is
/// applied in two passes: the script objects first, then, once the class
/// database knows the new classes, the objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    All,
    Script,
    Objects,
}

#[derive(Clone, Debug, Default)]
pub struct Applied {
    pub added: usize,
    pub replaced: usize,
    pub deleted: usize,
    /// Objects a `set` replaced whose base no longer matched what the delta
    /// was made against (another mod or a game update changed them first).
    pub drift: Vec<String>,
    pub warnings: Vec<String>,
}

/// Apply `delta` to the open package `ed`.
pub fn apply(db: &ClassDb, ed: &mut Editor, delta: &Delta, phase: Phase) -> Result<Applied> {
    apply_with(db, ed, delta, phase, None)
}

/// [`apply`], with the package as it was before any delta was applied
/// (`original`): payloads compressed against their base object (`from`) are
/// decompressed with the object from there, so they still apply after an
/// earlier delta changed the same object. Without it, the object as it is in
/// `ed` is used.
pub fn apply_with(db: &ClassDb, ed: &mut Editor, delta: &Delta, phase: Phase, original: Option<&Package>) -> Result<Applied> {
    let mut out = Applied::default();
    let original_paths = original.map(path_index);
    let mini = Package::parse(&delta.image)?;
    let mini_layout = delta.layout();
    let mini_none = none_name(&mini);
    let refs: Vec<Value> = delta.header["refs"].as_array().cloned().unwrap_or_default();
    let ops = delta.ops();
    let in_phase = |class: &str| match phase {
        Phase::All => true,
        Phase::Script => is_script_class(class),
        Phase::Objects => !is_script_class(class),
    };

    ed.flush();
    let mut paths = path_index(&ed.pkg);
    let target_layout = ed.layout();

    // Drift is judged against the package as it was before this delta: once
    // the deletes below run, objects that referred to a deleted one hash
    // differently even though nobody else changed them.
    let mut drifted: HashSet<String> = HashSet::new();
    for op in ops.iter().filter(|o| o.op == "set" && in_phase(&o.class)) {
        let (Some(base), Some(&t)) = (&op.base, paths.get(&op.path.to_ascii_lowercase())) else { continue };
        if let Some(h) = canonical(db, &ed.pkg, target_layout, t) {
            if hex(&h) != *base {
                drifted.insert(op.path.to_ascii_lowercase());
            }
        }
    }

    // Deletes first, so an object deleted and re-created elsewhere does not
    // clash with its old self.
    if phase != Phase::Objects {
        for op in ops.iter().filter(|o| o.op == "delete") {
            if let Some(&t) = paths.get(&op.path.to_ascii_lowercase()) {
                ed.delete(t)?;
                paths.remove(&op.path.to_ascii_lowercase());
                out.deleted += 1;
            } else {
                out.warnings.push(format!("{}: not in the package, nothing to delete", op.path));
            }
        }
    }

    // Pass 1: every changed object gets its export (existing, or appended),
    // whatever the phase, so a class applied now can point at the default
    // object whose payload only arrives in the next phase.
    let mut target_of: HashMap<usize, usize> = HashMap::new();
    for op in ops.iter().filter(|o| o.op != "delete") {
        let Some(k) = op.export else { continue };
        let key = op.path.to_ascii_lowercase();
        if op.op == "patch" && !paths.contains_key(&key) {
            // A patch changes properties of an object; without it there is nothing to change.
            if in_phase(&op.class) {
                out.warnings.push(format!("{}: not in the package, its property changes were skipped", op.path));
            }
            continue;
        }
        let t = match paths.get(&key) {
            Some(&t) => {
                if in_phase(&op.class) && (op.op == "set" || op.op == "patch") {
                    if drifted.contains(&key) {
                        out.drift.push(op.path.clone());
                    }
                    out.replaced += 1;
                }
                t
            }
            None => {
                let me = &mini.exports[k];
                let name = mini.object_name(PackageIndex::export(k));
                let export = ObjectExport { object_name: ed.pkg.intern_fname(&name), object_flags: me.object_flags, export_flags: me.export_flags, generation_net_object_count: me.generation_net_object_count.clone(), package_guid: me.package_guid, package_flags: me.package_flags, ..Default::default() };
                let t = ed.add_export(export, Vec::new());
                paths.insert(key, t);
                out.added += 1;
                t
            }
        };
        target_of.insert(k, t);
    }

    // References of the mini package in the target's coordinates.
    struct Resolver<'s> {
        mini: &'s Package,
        refs: &'s [Value],
        target_of: &'s HashMap<usize, usize>,
        memo: HashMap<PackageIndex, PackageIndex>,
        warnings: Vec<String>,
    }
    impl Resolver<'_> {
        fn resolve(&mut self, ed: &mut Editor, paths: &mut HashMap<String, usize>, idx: PackageIndex) -> PackageIndex {
            if idx.is_null() {
                return PackageIndex::NONE;
            }
            if let Some(&m) = self.memo.get(&idx) {
                return m;
            }
            let m = if let Some(k) = idx.export_index() {
                match self.target_of.get(&k) {
                    Some(&t) => PackageIndex::export(t),
                    // An object of another pass: already in the package by now.
                    None => paths.get(&self.mini.object_path(idx).to_ascii_lowercase()).map(|&t| PackageIndex::export(t)).unwrap_or(PackageIndex::NONE),
                }
            } else {
                let k = idx.import_index().unwrap();
                let desc = self.refs.get(k).cloned().unwrap_or(Value::Null);
                if desc["k"].as_str() == Some("export") {
                    let path = desc["path"].as_str().unwrap_or("");
                    let class = desc["class"].as_str().unwrap_or("");
                    match paths.get(&path.to_ascii_lowercase()) {
                        Some(&t) => PackageIndex::export(t),
                        None if class.eq_ignore_ascii_case("Package") => {
                            // A group the base lacks: recreate it under its outer.
                            let outer = self.resolve(ed, paths, self.mini.imports[k].outer);
                            let core = ed.pkg.add_import("Core", "Package", PackageIndex::NONE, "Core");
                            let class = ed.pkg.add_import("Core", "Class", core, "Package");
                            let flags = (0..ed.pkg.exports.len()).find(|&i| ed.pkg.is_class(PackageIndex::export(i), "Package")).map(|i| ed.pkg.exports[i].object_flags).unwrap_or(0);
                            let name = self.mini.object_name(idx);
                            let export = ObjectExport { object_name: ed.pkg.intern_fname(&name), class, outer, object_flags: flags, ..Default::default() };
                            let t = ed.add_export(export, Vec::new());
                            paths.insert(path.to_ascii_lowercase(), t);
                            PackageIndex::export(t)
                        }
                        None => {
                            self.warnings.push(format!("{path} ({class}) is not in the package; the reference was cleared"));
                            PackageIndex::NONE
                        }
                    }
                } else {
                    ed.ensure_import(self.mini, idx)
                }
            };
            self.memo.insert(idx, m);
            m
        }
    }
    let mut res = Resolver { mini: &mini, refs: &refs, target_of: &target_of, memo: HashMap::new(), warnings: Vec::new() };

    // Pass 2: table entries and payloads.
    let target_none = none_name(&ed.pkg);
    for op in ops.iter().filter(|o| o.op != "delete" && in_phase(&o.class)) {
        let Some(k) = op.export else { continue };
        let Some(&t) = target_of.get(&k) else { continue };
        // A payload compressed against the base object: that object must be
        // the one the delta was made from.
        let from_data: Option<Vec<u8>> = match (&op.from, delta.from.get(&k)) {
            (Some(f), Some(z)) => {
                let want = f["hash"].as_str().unwrap_or("");
                let size = f["size"].as_u64().unwrap_or(0) as usize;
                let dict = match (original, &original_paths) {
                    (Some(o), Some(op_paths)) => op_paths.get(&op.path.to_ascii_lowercase()).and_then(|&j| o.export_data.get(j)).cloned(),
                    _ => ed.pkg.export_data.get(t).cloned(),
                };
                match dict.filter(|d| bytes_hash(d) == want) {
                    Some(d) => Some(unpack_from(&d, z, size)?),
                    None => {
                        out.warnings.push(format!("{}: the mod was made from another version of this object (the game file differs); it was left unchanged", op.path));
                        out.replaced = out.replaced.saturating_sub(1);
                        out.drift.retain(|d| d != &op.path);
                        continue;
                    }
                }
            }
            _ => None,
        };
        if let Some(patch) = &op.patch {
            // The changed properties, in the target's names and references…
            let data = mini.export_data.get(k).map(|d| d.as_slice()).unwrap_or(&[]);
            let Parsed::Object(mut pm) = parse_export(db, &mini, mini_layout, k, data) else {
                out.warnings.push(format!("{}: the property changes could not be read", op.path));
                continue;
            };
            {
                struct PatchRemap<'s, 'r, 'e> {
                    res: &'s mut Resolver<'r>,
                    ed: &'s mut Editor<'e>,
                    paths: &'s mut HashMap<String, usize>,
                    names: HashMap<u32, u32>,
                }
                impl Visitor for PatchRemap<'_, '_, '_> {
                    fn name(&mut self, n: &mut FName) {
                        let mini = self.res.mini;
                        let idx = *self.names.entry(n.index).or_insert_with(|| self.ed.pkg.intern_name(mini.name_str(n.index)));
                        n.index = idx;
                    }
                    fn index(&mut self, i: &mut PackageIndex) {
                        *i = self.res.resolve(self.ed, self.paths, *i);
                    }
                }
                let mut wrapped = Parsed::Object(pm);
                let mut remap = PatchRemap { res: &mut res, ed, paths: &mut paths, names: HashMap::new() };
                for problem in visit_parsed(&mut wrapped, Some(mini_none.index), &mut remap) {
                    out.warnings.push(format!("{}: {problem}", op.path));
                }
                let Parsed::Object(o) = wrapped else { unreachable!() };
                pm = o;
            }
            // …merged into the object as it is now.
            let tdata = ed.pkg.export_data.get(t).cloned().unwrap_or_default();
            let Parsed::Object(mut pt) = parse_export(db, &ed.pkg, target_layout, t, &tdata) else {
                out.warnings.push(format!("{}: the object could not be read; its property changes were skipped", op.path));
                continue;
            };
            let key_of = |v: &Value| -> Option<(String, i32)> { Some((v[0].as_str()?.to_ascii_lowercase(), v[1].as_i64()? as i32)) };
            let remove: Vec<(String, i32)> = patch["remove"].as_array().map(|a| a.iter().filter_map(key_of).collect()).unwrap_or_default();
            let after: Vec<Option<(String, i32)>> = patch["after"].as_array().map(|a| a.iter().map(key_of).collect()).unwrap_or_default();
            let pkg = &ed.pkg;
            pt.properties.retain(|p| !remove.contains(&prop_key(pkg, p)));
            for (n, np) in pm.properties.into_iter().enumerate() {
                let k = prop_key(pkg, &np);
                if let Some(pos) = pt.properties.iter().position(|p| prop_key(pkg, p) == k) {
                    pt.properties[pos] = np;
                    continue;
                }
                let at = match after.get(n).cloned().flatten() {
                    Some(a) => pt.properties.iter().position(|p| prop_key(pkg, p) == a).map(|x| x + 1).unwrap_or(pt.properties.len()),
                    None => 0,
                };
                pt.properties.insert(at, np);
            }
            let bytes = write_parsed(&Parsed::Object(pt), target_layout, target_none);
            ed.set_raw(t, bytes)?;
            continue;
        }
        let me = mini.exports[k].clone();
        let mut table: Vec<PackageIndex> = [me.class, me.super_, me.outer, me.archetype].into_iter().map(|r| res.resolve(ed, &mut paths, r)).collect();
        // The cooker stores a class export's class (UClass) as a null index.
        if describe(&ed.pkg, table[0]) == "i|core|class|core.class" {
            table[0] = PackageIndex::NONE;
        }
        {
            let e = ed.export_mut(t)?;
            e.class = table[0];
            e.super_ = table[1];
            e.outer = table[2];
            e.archetype = table[3];
            e.object_flags = me.object_flags;
            e.export_flags = me.export_flags;
            e.generation_net_object_count = me.generation_net_object_count.clone();
            e.package_guid = me.package_guid;
            e.package_flags = me.package_flags;
        }
        // A shared payload: its own one-export package and reference list.
        let shared_unit = match &op.shared {
            Some(hash) => {
                let bytes = delta.shared.get(hash).ok_or_else(|| Error::Format(format!("{}: shared object {hash} is missing", op.path)))?;
                let tiny = Package::parse(bytes)?;
                let trefs: Vec<Value> = delta.header["ops"]
                    .as_array()
                    .and_then(|a| a.iter().find(|o| o["shared"]["hash"].as_str() == Some(hash.as_str())))
                    .and_then(|o| o["shared"]["refs"].as_array().cloned())
                    .unwrap_or_default();
                Some((tiny, trefs))
            }
            None => None,
        };
        struct Remap<'s, 'r, 'e> {
            res: &'s mut Resolver<'r>,
            ed: &'s mut Editor<'e>,
            paths: &'s mut HashMap<String, usize>,
            names: HashMap<u32, u32>,
        }
        impl Visitor for Remap<'_, '_, '_> {
            fn name(&mut self, n: &mut FName) {
                let mini = self.res.mini;
                let idx = *self.names.entry(n.index).or_insert_with(|| self.ed.pkg.intern_name(mini.name_str(n.index)));
                n.index = idx;
            }
            fn index(&mut self, i: &mut PackageIndex) {
                *i = self.res.resolve(self.ed, self.paths, *i);
            }
        }
        let bytes = match &shared_unit {
            Some((tiny, trefs)) => {
                let self_of: HashMap<usize, usize> = HashMap::from([(0, t)]);
                let mut tres = Resolver { mini: tiny, refs: trefs, target_of: &self_of, memo: HashMap::new(), warnings: Vec::new() };
                let data = tiny.export_data.first().map(|d| d.as_slice()).unwrap_or(&[]);
                let mut p = parse_export(db, tiny, mini_layout, 0, data);
                let mut remap = Remap { res: &mut tres, ed, paths: &mut paths, names: HashMap::new() };
                for problem in visit_parsed(&mut p, Some(none_name(tiny).index), &mut remap) {
                    out.warnings.push(format!("{}: {problem}", op.path));
                }
                out.warnings.append(&mut tres.warnings);
                write_parsed(&p, target_layout, target_none)
            }
            None => {
                let streamed = op.stream.and_then(|(s, o, n)| delta.streams.get(s)?.get(o..o + n));
                let data = from_data.as_deref().or(streamed).unwrap_or_else(|| mini.export_data.get(k).map(|d| d.as_slice()).unwrap_or(&[]));
                let mut p = parse_export(db, &mini, mini_layout, k, data);
                let mut remap = Remap { res: &mut res, ed, paths: &mut paths, names: HashMap::new() };
                for problem in visit_parsed(&mut p, Some(mini_none.index), &mut remap) {
                    out.warnings.push(format!("{}: {problem}", op.path));
                }
                write_parsed(&p, target_layout, target_none)
            }
        };
        ed.set_raw(t, bytes)?;
    }
    out.warnings.append(&mut res.warnings);
    Ok(out)
}

/// Objects two deltas both touch (lower-case paths): what the launcher
/// reports as a conflict.
pub fn overlap(a: &Delta, b: &Delta) -> Vec<String> {
    let keys: HashSet<String> = a.ops().into_iter().map(|o| o.path.to_ascii_lowercase()).collect();
    let mut out: Vec<String> = b.ops().into_iter().map(|o| o.path.to_ascii_lowercase()).filter(|p| keys.contains(p)).collect();
    out.sort();
    out.dedup();
    out
}

/// Every object path of `result` whose canonical form differs from
/// `expected`'s, plus objects one has and the other lacks: the round-trip
/// check (`apply(create(base, edited), base)` must equal `edited`).
pub fn compare(db: &ClassDb, expected: &Package, result: &Package) -> Vec<String> {
    let (le, lr) = (package_layout(expected), package_layout(result));
    let pe = path_index(expected);
    let pr = path_index(result);
    let mut diffs = Vec::new();
    for (path, &i) in &pe {
        match pr.get(path) {
            None => diffs.push(format!("{path}: missing from the result")),
            Some(&j) => {
                if canonical(db, expected, le, i) != canonical(db, result, lr, j) {
                    diffs.push(format!("{path}: differs"));
                }
            }
        }
    }
    for path in pr.keys() {
        if !pe.contains_key(path) {
            diffs.push(format!("{path}: unexpected in the result"));
        }
    }
    diffs.sort();
    diffs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta_with(hash: &str, blob: &[u8]) -> Delta {
        let header = json!({ "format": "tdelta", "kind": "upk", "target": "ChaosGame/CookedPCConsole/A.upk", "refs": [], "ops": [
            { "op": "set", "path": "a.mesh", "class": "SkeletalMesh", "export": 0, "shared": { "hash": hash, "refs": [] } },
        ] });
        Delta { header, image: vec![1, 2, 3], shared: BTreeMap::from([(hash.to_string(), blob.to_vec())]), ..Default::default() }
    }

    #[test]
    fn plain_deltas_stay_version_1() {
        let d = Delta { header: json!({ "ops": [{ "op": "set", "path": "a", "class": "Texture2D", "export": 0 }] }), image: vec![9], ..Default::default() };
        let bytes = d.to_bytes();
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 1);
        assert_eq!(Delta::from_bytes(&bytes).unwrap().image, vec![9]);
    }

    #[test]
    fn compression_round_trips() {
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let packed = pack(&data);
        assert!(packed.len() < data.len() / 10 && packed.starts_with(XZ_MAGIC));
        assert_eq!(unpack(&packed).unwrap(), data);
        // Not worth it: kept as it is.
        assert_eq!(pack(b"abc"), b"abc".to_vec());
        // The fast mode (zstd) reads back the same way.
        let fast = pack_with(&data, Compression::Fast);
        assert!(fast.starts_with(ZSTD_MAGIC) && fast.len() < data.len() / 10);
        assert_eq!(unpack(&fast).unwrap(), data);
        assert_eq!(pack_all_with(&[(&data[..], Compression::Fast), (&data[..], Compression::Small)]).iter().map(|p| unpack(p).unwrap()).collect::<Vec<_>>(), vec![data.clone(), data.clone()]);
        // Against a base: only the difference costs.
        let mut edited = data.clone();
        edited[1000..1100].fill(7);
        let z = pack_from(&data, &edited).unwrap();
        assert!(z.len() < 1000);
        assert_eq!(unpack_from(&data, &z, edited.len()).unwrap(), edited);
    }

    #[test]
    fn shared_objects_embed_or_live_in_shared_folder() {
        let blob = b"mesh bytes".to_vec();
        let hash = hex(&Sha256::digest(&blob));
        let d = delta_with(&hash, &blob);

        // Embedded: one self-contained version 2 file.
        let embedded = Delta::from_bytes(&d.to_bytes()).unwrap();
        assert_eq!(embedded.shared.get(&hash), Some(&blob));

        // External: the delta under deltas/…, the object file in the mod's shared/.
        let root = std::env::temp_dir().join(format!("tdelta-shared-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let deltas = root.join("deltas").join("ChaosGame").join("CookedPCConsole");
        std::fs::create_dir_all(&deltas).unwrap();
        let file = deltas.join("A.upk.tdelta");
        d.write_shared(&file, root.join("shared")).unwrap();
        assert!(root.join("shared").join(format!("{hash}.{SHARED_EXT}")).is_file());
        assert!(Delta::from_bytes(&std::fs::read(&file).unwrap()).unwrap().shared.is_empty());
        let read = Delta::read(&file).unwrap();
        assert_eq!(read.shared.get(&hash), Some(&blob));

        // A tampered object file is refused.
        std::fs::write(root.join("shared").join(format!("{hash}.{SHARED_EXT}")), b"other").unwrap();
        assert!(Delta::read(&file).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
