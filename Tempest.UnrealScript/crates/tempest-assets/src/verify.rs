//! Round-trip and coverage check over a directory of packages.
//!
//! For every package: expand it, parse every export, write every export
//! back and compare; then rebuild the whole package image and compare the
//! tables and payload layout too. Coverage counts how much of each export
//! is understood: header parsed, every property value typed, native tail
//! empty.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tempest_unrealscript::par;
use tempest_upk::io::PackageIndex;
use tempest_upk::{PackageFile, WriteOptions};

use crate::object::{none_name, package_layout, parse_export, write_parsed, Parsed};
use crate::reflect::ClassDb;

#[derive(Clone, Debug, Default)]
pub struct ClassStats {
    pub exports: usize,
    /// Header could not be parsed.
    pub raw: usize,
    /// Header parsed but a property value stayed raw.
    pub partial_values: usize,
    /// Header parsed, native tail non-empty.
    pub with_tail: usize,
    pub tail_bytes: u64,
    /// Fully understood (typed values, no tail).
    pub full: usize,
    pub mismatches: usize,
}

#[derive(Clone, Debug, Default)]
pub struct PackageReport {
    pub path: PathBuf,
    pub name: String,
    pub error: Option<String>,
    pub exports: usize,
    pub mismatches: usize,
    pub raw: usize,
    pub partial_values: usize,
    pub with_tail: usize,
    pub full: usize,
    /// Tables + payload layout rebuilt identically (uncompressed image).
    pub image_identical: bool,
    /// Re-compressing reproduces the file byte-for-byte (only checked for
    /// compressed inputs).
    pub compressed_identical: Option<bool>,
    pub per_class: BTreeMap<String, ClassStats>,
    /// First few mismatching exports, for investigation.
    pub mismatch_samples: Vec<String>,
    pub raw_samples: Vec<String>,
}

pub fn verify_package(db: &ClassDb, path: &Path, check_compressed: bool) -> PackageReport {
    let mut rep = PackageReport { path: path.to_path_buf(), name: path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), ..Default::default() };
    let pf = match PackageFile::open(path) {
        Ok(p) => p,
        Err(e) => {
            rep.error = Some(e.to_string());
            return rep;
        }
    };
    let mut pkg = match pf.load() {
        Ok(p) => p,
        Err(e) => {
            rep.error = Some(e.to_string());
            return rep;
        }
    };
    let none = none_name(&pkg);
    let layout = package_layout(&pkg);
    rep.exports = pkg.exports.len();
    for i in 0..pkg.exports.len() {
        let data = &pkg.export_data[i];
        let parsed = parse_export(db, &pkg, layout, i, data);
        let class_name = pkg.class_name_of(PackageIndex::export(i)).to_ascii_lowercase();
        let cs = rep.per_class.entry(class_name).or_default();
        cs.exports += 1;
        match &parsed {
            Parsed::Empty | Parsed::Script(_) => cs.full += 1,
            Parsed::Raw { error, .. } => {
                cs.raw += 1;
                if rep.raw_samples.len() < 8 {
                    rep.raw_samples.push(format!("{} : {error}", pkg.full_name(PackageIndex::export(i))));
                }
            }
            Parsed::Object(o) => {
                let partial = o.has_raw_values();
                if partial {
                    cs.partial_values += 1;
                }
                if o.native.is_raw() {
                    cs.with_tail += 1;
                    cs.tail_bytes += o.native.raw_len() as u64;
                }
                if !partial && !o.native.is_raw() {
                    cs.full += 1;
                }
            }
        }
        let re = write_parsed(&parsed, layout, none);
        if &re != data {
            cs.mismatches += 1;
            if rep.mismatch_samples.len() < 8 {
                rep.mismatch_samples.push(format!("{} ({} vs {} bytes)", pkg.full_name(PackageIndex::export(i)), re.len(), data.len()));
            }
        }
    }
    for cs in rep.per_class.values() {
        rep.mismatches += cs.mismatches;
        rep.raw += cs.raw;
        rep.partial_values += cs.partial_values;
        rep.with_tail += cs.with_tail;
        rep.full += cs.full;
    }

    // Container round trip: uncompressed image from the name table on.
    let image_len = pf.image_len();
    let was_compressed = pkg.was_compressed;
    let summary_before = pkg.summary.clone();
    let out = pkg.to_bytes(&WriteOptions { compress: false, obscure: false });
    let from = summary_before.name_offset.max(0) as usize;
    rep.image_identical = match pf.read_range(from, image_len.saturating_sub(from)) {
        Ok(orig) => out.len() == image_len && out[from..] == orig[..],
        Err(_) => false,
    };
    if check_compressed && was_compressed {
        pkg.summary = summary_before;
        let obscure = pkg.summary.compression_flags & tempest_upk::flags::compression::OBSCURED != 0;
        let comp = pkg.to_bytes(&WriteOptions { compress: true, obscure });
        let orig = std::fs::read(path).unwrap_or_default();
        rep.compressed_identical = Some(comp == orig);
    }
    rep
}

/// Verify every package of `dir` (parallel), reports in name order.
pub fn verify_dir(db: &ClassDb, dir: &Path, check_compressed: bool) -> Vec<PackageReport> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| matches!(p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref(), Some("upk") | Some("u")))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    par::map(&files, |f| verify_package(db, f, check_compressed))
}

/// Merge the per-class stats of several reports.
pub fn merge_classes(reports: &[PackageReport]) -> BTreeMap<String, ClassStats> {
    let mut out: BTreeMap<String, ClassStats> = BTreeMap::new();
    for r in reports {
        for (k, v) in &r.per_class {
            let e = out.entry(k.clone()).or_default();
            e.exports += v.exports;
            e.raw += v.raw;
            e.partial_values += v.partial_values;
            e.with_tail += v.with_tail;
            e.tail_bytes += v.tail_bytes;
            e.full += v.full;
            e.mismatches += v.mismatches;
        }
    }
    out
}
