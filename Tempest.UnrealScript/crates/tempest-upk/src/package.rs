//! `FPackageFileSummary`, the name/import/export/depends tables and the
//! export payloads of a UE3 package, with LZO (+ Hi-Rez XOR) decompression on
//! load and uncompressed or compressed output on save.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use crate::flags::{self, compression, pkg};
use crate::io::{Error, FName, Guid, PackageIndex, Reader, Result, Writer};
use crate::lzo;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GenerationInfo {
    pub export_count: i32,
    pub name_count: i32,
    pub net_object_count: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompressedChunk {
    pub uncompressed_offset: i32,
    pub uncompressed_size: i32,
    pub compressed_offset: i32,
    pub compressed_size: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextureType {
    pub size_x: i32,
    pub size_y: i32,
    pub num_mips: i32,
    pub format: u32,
    pub tex_create_flags: u32,
    pub export_indices: Vec<i32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub tag: u32,
    pub file_version: u16,
    pub licensee_version: u16,
    pub total_header_size: i32,
    pub folder_name: String,
    pub package_flags: u32,
    pub name_count: i32,
    pub name_offset: i32,
    pub export_count: i32,
    pub export_offset: i32,
    pub import_count: i32,
    pub import_offset: i32,
    pub depends_offset: i32,
    pub import_export_guids_offset: i32,
    pub import_guids_count: i32,
    pub export_guids_count: i32,
    pub thumbnail_table_offset: i32,
    pub guid: Guid,
    pub generations: Vec<GenerationInfo>,
    pub engine_version: u32,
    pub cooked_content_version: u32,
    pub compression_flags: u32,
    pub compressed_chunks: Vec<CompressedChunk>,
    pub package_source: u32,
    pub additional_packages_to_cook: Vec<String>,
    pub texture_allocations: Vec<TextureType>,
}

impl Summary {
    pub fn read(r: &mut Reader) -> Result<Summary> {
        let tag = r.u32()?;
        if tag != flags::PACKAGE_FILE_TAG {
            return Err(Error::BadTag(tag));
        }
        let mut s = Summary { tag, ..Default::default() };
        s.file_version = r.u16()?;
        s.licensee_version = r.u16()?;
        s.total_header_size = r.i32()?;
        s.folder_name = r.fstring()?;
        s.package_flags = r.u32()?;
        s.name_count = r.i32()?;
        s.name_offset = r.i32()?;
        s.export_count = r.i32()?;
        s.export_offset = r.i32()?;
        s.import_count = r.i32()?;
        s.import_offset = r.i32()?;
        s.depends_offset = r.i32()?;
        if s.file_version >= 623 {
            s.import_export_guids_offset = r.i32()?;
            s.import_guids_count = r.i32()?;
            s.export_guids_count = r.i32()?;
        } else {
            s.import_export_guids_offset = -1;
        }
        if s.file_version >= 584 {
            s.thumbnail_table_offset = r.i32()?;
        }
        s.guid = r.guid()?;
        s.generations = r.array(|r| {
            Ok(GenerationInfo {
                export_count: r.i32()?,
                name_count: r.i32()?,
                net_object_count: r.i32()?,
            })
        })?;
        s.engine_version = r.u32()?;
        s.cooked_content_version = r.u32()?;
        s.compression_flags = r.u32()?;
        s.compressed_chunks = r.array(|r| {
            Ok(CompressedChunk {
                uncompressed_offset: r.i32()?,
                uncompressed_size: r.i32()?,
                compressed_offset: r.i32()?,
                compressed_size: r.i32()?,
            })
        })?;
        s.package_source = r.u32()?;
        if s.file_version >= 516 {
            s.additional_packages_to_cook = r.array(|r| r.fstring())?;
        }
        if s.file_version >= 767 {
            s.texture_allocations = r.array(|r| {
                Ok(TextureType {
                    size_x: r.i32()?,
                    size_y: r.i32()?,
                    num_mips: r.i32()?,
                    format: r.u32()?,
                    tex_create_flags: r.u32()?,
                    export_indices: r.array(|r| r.i32())?,
                })
            })?;
        }
        Ok(s)
    }

    pub fn write(&self, w: &mut Writer) {
        w.u32(flags::PACKAGE_FILE_TAG);
        w.u16(self.file_version);
        w.u16(self.licensee_version);
        w.i32(self.total_header_size);
        w.fstring(&self.folder_name);
        w.u32(self.package_flags);
        w.i32(self.name_count);
        w.i32(self.name_offset);
        w.i32(self.export_count);
        w.i32(self.export_offset);
        w.i32(self.import_count);
        w.i32(self.import_offset);
        w.i32(self.depends_offset);
        if self.file_version >= 623 {
            w.i32(self.import_export_guids_offset);
            w.i32(self.import_guids_count);
            w.i32(self.export_guids_count);
        }
        if self.file_version >= 584 {
            w.i32(self.thumbnail_table_offset);
        }
        w.guid(&self.guid);
        w.array(&self.generations, |w, g| {
            w.i32(g.export_count);
            w.i32(g.name_count);
            w.i32(g.net_object_count);
        });
        w.u32(self.engine_version);
        w.u32(self.cooked_content_version);
        w.u32(self.compression_flags);
        w.array(&self.compressed_chunks, |w, c| {
            w.i32(c.uncompressed_offset);
            w.i32(c.uncompressed_size);
            w.i32(c.compressed_offset);
            w.i32(c.compressed_size);
        });
        w.u32(self.package_source);
        if self.file_version >= 516 {
            w.array(&self.additional_packages_to_cook, |w, s| w.fstring(s));
        }
        if self.file_version >= 767 {
            w.array(&self.texture_allocations, |w, t| {
                w.i32(t.size_x);
                w.i32(t.size_y);
                w.i32(t.num_mips);
                w.u32(t.format);
                w.u32(t.tex_create_flags);
                w.array(&t.export_indices, |w, i| w.i32(*i));
            });
        }
    }

    pub fn is_cooked(&self) -> bool {
        self.package_flags & pkg::COOKED != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameEntry {
    pub name: String,
    pub flags: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectImport {
    pub class_package: FName,
    pub class_name: FName,
    pub outer: PackageIndex,
    pub object_name: FName,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectExport {
    pub class: PackageIndex,
    pub super_: PackageIndex,
    pub outer: PackageIndex,
    pub object_name: FName,
    pub archetype: PackageIndex,
    pub object_flags: u64,
    pub serial_size: i32,
    pub serial_offset: i32,
    pub export_flags: u32,
    pub generation_net_object_count: Vec<i32>,
    pub package_guid: Guid,
    pub package_flags: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LevelGuids {
    pub level_name: String,
    pub guids: Vec<Guid>,
}

/// A fully parsed package. Export payloads are owned copies so that objects
/// can be rewritten and the package re-emitted.
#[derive(Clone, Debug, Default)]
pub struct Package {
    pub summary: Summary,
    pub names: Vec<NameEntry>,
    pub imports: Vec<ObjectImport>,
    pub exports: Vec<ObjectExport>,
    pub depends: Vec<Vec<i32>>,
    pub import_guids: Vec<LevelGuids>,
    pub export_guids: Vec<(Guid, i32)>,
    /// Raw bytes between the end of the parsed header tables and
    /// `TotalHeaderSize` (thumbnail table etc.), kept verbatim.
    pub header_tail: Vec<u8>,
    pub export_data: Vec<Vec<u8>>,
    /// Bytes after the last export payload (normally empty).
    pub trailing: Vec<u8>,
    /// Whether the on-disk file was compressed; the writer can re-compress.
    pub was_compressed: bool,
    name_lookup: HashMap<String, u32>,
}

impl Package {
    pub fn load(path: impl AsRef<Path>) -> Result<Package> {
        let bytes = std::fs::read(path)?;
        Package::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Package> {
        Package::parse_image(bytes, true)
    }

    /// Summary and header tables only: `export_data` stays empty and only the
    /// compressed chunks covering the header are expanded. For scanning a
    /// directory of packages without decompressing their content.
    pub fn load_tables(path: impl AsRef<Path>) -> Result<Package> {
        let bytes = std::fs::read(path)?;
        Package::parse_image(&bytes, false)
    }

    fn parse_image(bytes: &[u8], payloads: bool) -> Result<Package> {
        let mut r = Reader::new(bytes);
        let summary = Summary::read(&mut r)?;
        let image: Vec<u8>;
        let was_compressed = !summary.compressed_chunks.is_empty();
        let data: &[u8] = if was_compressed {
            let limit = if payloads { usize::MAX } else { summary.total_header_size.max(0) as usize };
            image = decompress_image(bytes, &summary, limit)?;
            &image
        } else {
            bytes
        };
        let mut pkg = Package { summary, was_compressed, ..Default::default() };
        pkg.parse_tables(data, payloads)?;
        Ok(pkg)
    }

    /// Tables from an expanded header image (`total_header_size` bytes of the
    /// uncompressed file) and its summary; `export_data` stays empty.
    pub fn parse_header(header: &[u8], summary: Summary) -> Result<Package> {
        let mut pkg = Package { summary, ..Default::default() };
        pkg.parse_tables(header, false)?;
        Ok(pkg)
    }

    /// Only the summary, from the start of a file (no chunk is expanded).
    pub fn read_summary(path: impl AsRef<Path>) -> Result<Summary> {
        use std::io::Read;
        let mut f = std::fs::File::open(path)?;
        let mut buf = Vec::with_capacity(64 * 1024);
        f.by_ref().take(4 * 1024 * 1024).read_to_end(&mut buf)?;
        Summary::read(&mut Reader::new(&buf))
    }

    fn parse_tables(&mut self, data: &[u8], payloads: bool) -> Result<()> {
        let s = &self.summary;
        let mut r = Reader::new(data);

        r.seek(s.name_offset as usize);
        for _ in 0..s.name_count {
            let name = r.fstring()?;
            let flags = r.u64()?;
            self.names.push(NameEntry { name, flags });
        }

        r.seek(s.import_offset as usize);
        for _ in 0..s.import_count {
            self.imports.push(ObjectImport {
                class_package: r.fname()?,
                class_name: r.fname()?,
                outer: r.index()?,
                object_name: r.fname()?,
            });
        }

        r.seek(s.export_offset as usize);
        for _ in 0..s.export_count {
            let class = r.index()?;
            let super_ = r.index()?;
            let outer = r.index()?;
            let object_name = r.fname()?;
            let archetype = r.index()?;
            let object_flags = r.u64()?;
            let serial_size = r.i32()?;
            let serial_offset = r.i32()?;
            let export_flags = r.u32()?;
            let generation_net_object_count = r.array(|r| r.i32())?;
            let package_guid = r.guid()?;
            let package_flags = r.u32()?;
            self.exports.push(ObjectExport {
                class,
                super_,
                outer,
                object_name,
                archetype,
                object_flags,
                serial_size,
                serial_offset,
                export_flags,
                generation_net_object_count,
                package_guid,
                package_flags,
            });
        }

        if s.depends_offset > 0 {
            r.seek(s.depends_offset as usize);
            for _ in 0..s.export_count {
                self.depends.push(r.array(|r| r.i32())?);
            }
        }

        let mut tables_end = r.pos();
        if s.import_export_guids_offset > 0 {
            r.seek(s.import_export_guids_offset as usize);
            for _ in 0..s.import_guids_count {
                let level_name = r.fstring()?;
                let guids = r.array(|r| r.guid())?;
                self.import_guids.push(LevelGuids { level_name, guids });
            }
            for _ in 0..s.export_guids_count {
                let g = r.guid()?;
                let idx = r.i32()?;
                self.export_guids.push((g, idx));
            }
            tables_end = r.pos();
        }
        let header_end = s.total_header_size as usize;
        if header_end > tables_end && header_end <= data.len() {
            self.header_tail = data[tables_end..header_end].to_vec();
        }
        if !payloads {
            self.rebuild_name_lookup();
            return Ok(());
        }

        let mut max_end = header_end;
        for e in &self.exports {
            let start = e.serial_offset as usize;
            let end = start + e.serial_size as usize;
            if end > data.len() {
                return Err(Error::Format(format!(
                    "export payload {}..{} exceeds package size {}",
                    start,
                    end,
                    data.len()
                )));
            }
            self.export_data.push(data[start..end].to_vec());
            max_end = max_end.max(end);
        }
        if max_end < data.len() {
            self.trailing = data[max_end..].to_vec();
        }
        self.rebuild_name_lookup();
        Ok(())
    }

    pub fn rebuild_name_lookup(&mut self) {
        self.name_lookup.clear();
        for (i, n) in self.names.iter().enumerate() {
            self.name_lookup.entry(n.name.to_ascii_lowercase()).or_insert(i as u32);
        }
    }

    // ----- name helpers -------------------------------------------------

    pub fn name_str(&self, index: u32) -> &str {
        self.names.get(index as usize).map(|n| n.name.as_str()).unwrap_or("<bad-name>")
    }

    /// Render an `FName` with its instance suffix.
    pub fn fname_to_string(&self, n: FName) -> String {
        let base = self.name_str(n.index);
        if n.number == 0 {
            base.to_string()
        } else {
            format!("{}_{}", base, n.number - 1)
        }
    }

    pub fn find_name(&self, name: &str) -> Option<u32> {
        self.name_lookup.get(&name.to_ascii_lowercase()).copied()
    }

    /// Find or append a name-table entry (case-insensitive like `FName`).
    pub fn intern_name(&mut self, name: &str) -> u32 {
        if let Some(i) = self.find_name(name) {
            return i;
        }
        let i = self.names.len() as u32;
        // 0x0007001000000000: RF_TagExp | RF_LoadForClient | RF_LoadForServer | RF_LoadForEdit
        self.names.push(NameEntry { name: name.to_string(), flags: 0x0007_0010_0000_0000 });
        self.name_lookup.insert(name.to_ascii_lowercase(), i);
        i
    }

    /// Build an `FName` for `Name` or `Name_N` text.
    pub fn intern_fname(&mut self, text: &str) -> FName {
        let (base, number) = split_name_number(text);
        FName { index: self.intern_name(base), number }
    }

    // ----- editing ------------------------------------------------------

    /// Find or append an import (`ClassPackage.ClassName` of `outer.name`).
    pub fn add_import(&mut self, class_package: &str, class_name: &str, outer: PackageIndex, name: &str) -> PackageIndex {
        let object_name = self.intern_fname(name);
        let class_package = self.intern_fname(class_package);
        let class_name = self.intern_fname(class_name);
        if let Some(i) = self.imports.iter().position(|i| i.object_name == object_name && i.outer == outer && i.class_name == class_name && i.class_package == class_package) {
            return PackageIndex::import(i);
        }
        self.imports.push(ObjectImport { class_package, class_name, outer, object_name });
        PackageIndex::import(self.imports.len() - 1)
    }

    /// Append an export with its payload; `serial_offset`/`serial_size` are
    /// assigned on save. Returns its index.
    pub fn add_export(&mut self, export: ObjectExport, data: Vec<u8>) -> PackageIndex {
        while self.export_data.len() < self.exports.len() {
            self.export_data.push(Vec::new());
        }
        self.exports.push(export);
        self.export_data.push(data);
        self.depends.push(Vec::new());
        PackageIndex::export(self.exports.len() - 1)
    }

    /// Replace the payload of export `i`.
    pub fn set_export_data(&mut self, i: usize, data: Vec<u8>) {
        while self.export_data.len() < self.exports.len() {
            self.export_data.push(Vec::new());
        }
        self.exports[i].serial_size = data.len() as i32;
        self.export_data[i] = data;
    }

    /// Does any import or export reference `idx` as class, super, outer or archetype?
    pub fn table_refs_to(&self, idx: PackageIndex) -> usize {
        let mut n = 0;
        for i in &self.imports {
            if i.outer == idx {
                n += 1;
            }
        }
        for e in &self.exports {
            n += [e.class, e.super_, e.outer, e.archetype].iter().filter(|&&r| r == idx).count();
        }
        n
    }

    // ----- object helpers -----------------------------------------------

    pub fn object_name(&self, idx: PackageIndex) -> String {
        if let Some(e) = idx.export_index() {
            self.exports.get(e).map(|e| self.fname_to_string(e.object_name)).unwrap_or_default()
        } else if let Some(i) = idx.import_index() {
            self.imports.get(i).map(|i| self.fname_to_string(i.object_name)).unwrap_or_default()
        } else {
            "None".to_string()
        }
    }

    pub fn object_outer(&self, idx: PackageIndex) -> PackageIndex {
        if let Some(e) = idx.export_index() {
            self.exports.get(e).map(|e| e.outer).unwrap_or_default()
        } else if let Some(i) = idx.import_index() {
            self.imports.get(i).map(|i| i.outer).unwrap_or_default()
        } else {
            PackageIndex::NONE
        }
    }

    /// Dotted path (`Outer.Outer.Name`), without the class prefix.
    pub fn object_path(&self, idx: PackageIndex) -> String {
        if idx.is_null() {
            return "None".to_string();
        }
        let mut parts = vec![self.object_name(idx)];
        let mut cur = self.object_outer(idx);
        let mut guard = 0;
        while !cur.is_null() && guard < 64 {
            parts.push(self.object_name(cur));
            cur = self.object_outer(cur);
            guard += 1;
        }
        parts.reverse();
        parts.join(".")
    }

    /// Class name of an export or import.
    pub fn class_name_of(&self, idx: PackageIndex) -> String {
        if let Some(e) = idx.export_index() {
            let c = self.exports[e].class;
            if c.is_null() {
                "Class".to_string()
            } else {
                self.object_name(c)
            }
        } else if let Some(i) = idx.import_index() {
            self.fname_to_string(self.imports[i].class_name)
        } else {
            "None".to_string()
        }
    }

    pub fn full_name(&self, idx: PackageIndex) -> String {
        format!("{} {}", self.class_name_of(idx), self.object_path(idx))
    }

    /// Outermost package name for an index.
    pub fn outermost_name(&self, idx: PackageIndex) -> String {
        let mut cur = idx;
        let mut guard = 0;
        while !self.object_outer(cur).is_null() && guard < 64 {
            cur = self.object_outer(cur);
            guard += 1;
        }
        self.object_name(cur)
    }

    /// Find an export by path (`Outer.Name`) and optional class name.
    pub fn find_export(&self, path: &str, class: Option<&str>) -> Option<usize> {
        (0..self.exports.len()).find(|&i| {
            let idx = PackageIndex::export(i);
            self.object_path(idx).eq_ignore_ascii_case(path)
                && class.is_none_or(|c| self.class_name_of(idx).eq_ignore_ascii_case(c))
        })
    }

    pub fn find_import(&self, path: &str, class: Option<&str>) -> Option<usize> {
        (0..self.imports.len()).find(|&i| {
            let idx = PackageIndex::import(i);
            self.object_path(idx).eq_ignore_ascii_case(path)
                && class.is_none_or(|c| self.class_name_of(idx).eq_ignore_ascii_case(c))
        })
    }

    /// Does `idx` have `class_name` as its class (by name only)?
    pub fn is_class(&self, idx: PackageIndex, class_name: &str) -> bool {
        self.class_name_of(idx).eq_ignore_ascii_case(class_name)
    }

    /// The package name (first export's outermost, or the folder name).
    pub fn package_name(&self) -> String {
        for i in 0..self.exports.len() {
            let n = self.outermost_name(PackageIndex::export(i));
            if !n.is_empty() {
                return n;
            }
        }
        String::new()
    }

    // ----- writing ------------------------------------------------------

    /// Serialise the package. `compress` re-applies LZO (and the Hi-Rez XOR
    /// if `obscure` is set) the way the cooker does.
    pub fn to_bytes(&mut self, opts: &WriteOptions) -> Vec<u8> {
        self.summary.name_count = self.names.len() as i32;
        self.summary.import_count = self.imports.len() as i32;
        self.summary.export_count = self.exports.len() as i32;
        self.summary.import_guids_count = self.import_guids.len() as i32;
        self.summary.export_guids_count = self.export_guids.len() as i32;
        self.summary.file_version = self.summary.file_version.max(1);
        // The header is laid out without a chunk table: offsets of a compressed
        // package are in the coordinates of its uncompressed form (the cooker
        // adds the chunk table afterwards, see `compress_image`).
        self.summary.compressed_chunks.clear();
        if !opts.compress {
            self.summary.compression_flags = 0;
            self.summary.package_flags &= !pkg::STORE_COMPRESSED;
        }
        while self.depends.len() < self.exports.len() {
            self.depends.push(Vec::new());
        }
        self.depends.truncate(self.exports.len());

        // Pass 1: lay out the header with placeholder offsets.
        let (header, offsets) = self.write_header();
        let mut body = Writer::new();
        body.bytes(&header);
        let header_len = body.tell();

        // Export payloads.
        let mut serial = Vec::with_capacity(self.exports.len());
        for d in &self.export_data {
            // `SavePackage` leaves `SerialOffset` 0 for exports without data
            // (the cooker's placeholders).
            serial.push((if d.is_empty() { 0 } else { body.tell() as i32 }, d.len() as i32));
            body.bytes(d);
        }
        body.bytes(&self.trailing);
        for (e, (off, size)) in self.exports.iter_mut().zip(serial) {
            e.serial_offset = off;
            e.serial_size = size;
        }
        self.summary.total_header_size = header_len as i32;
        if let Some(g) = self.summary.generations.last_mut() {
            g.export_count = self.exports.len() as i32;
            g.name_count = self.names.len() as i32;
        }

        // Pass 2: real header (same size as pass 1).
        let (header2, _) = self.write_header();
        debug_assert_eq!(header2.len(), header_len);
        let mut out = body.into_inner();
        out[..header_len].copy_from_slice(&header2);
        let _ = offsets;

        if opts.compress {
            let ranges: Vec<(i32, i32)> = self.exports.iter().map(|e| (e.serial_offset, e.serial_size)).collect();
            compress_image(&out, &mut self.summary, opts.obscure, &ranges)
        } else {
            out
        }
    }

    pub fn save(&mut self, path: impl AsRef<Path>, opts: &WriteOptions) -> Result<()> {
        let bytes = self.to_bytes(opts);
        std::fs::write(path, bytes)?;
        Ok(())
    }

    fn write_header(&mut self) -> (Vec<u8>, HeaderOffsets) {
        let mut w = Writer::new();
        self.summary.write(&mut w);
        let name_offset = w.tell() as i32;
        for n in &self.names {
            w.fstring(&n.name);
            w.u64(n.flags);
        }
        let import_offset = w.tell() as i32;
        for i in &self.imports {
            w.fname(i.class_package);
            w.fname(i.class_name);
            w.index(i.outer);
            w.fname(i.object_name);
        }
        let export_offset = w.tell() as i32;
        for e in &self.exports {
            w.index(e.class);
            w.index(e.super_);
            w.index(e.outer);
            w.fname(e.object_name);
            w.index(e.archetype);
            w.u64(e.object_flags);
            w.i32(e.serial_size);
            w.i32(e.serial_offset);
            w.u32(e.export_flags);
            w.array(&e.generation_net_object_count, |w, v| w.i32(*v));
            w.guid(&e.package_guid);
            w.u32(e.package_flags);
        }
        let depends_offset = w.tell() as i32;
        for d in &self.depends {
            w.array(d, |w, v| w.i32(*v));
        }
        let guids_offset = w.tell() as i32;
        for lg in &self.import_guids {
            w.fstring(&lg.level_name);
            w.array(&lg.guids, |w, g| w.guid(g));
        }
        for (g, i) in &self.export_guids {
            w.guid(g);
            w.i32(*i);
        }
        w.bytes(&self.header_tail);
        let offsets = HeaderOffsets { name_offset, import_offset, export_offset, depends_offset, guids_offset };
        self.summary.name_offset = name_offset;
        self.summary.import_offset = import_offset;
        self.summary.export_offset = export_offset;
        self.summary.depends_offset = depends_offset;
        if self.summary.file_version >= 623 {
            self.summary.import_export_guids_offset = guids_offset;
        }
        (w.into_inner(), offsets)
    }
}

struct HeaderOffsets {
    #[allow(dead_code)]
    name_offset: i32,
    #[allow(dead_code)]
    import_offset: i32,
    #[allow(dead_code)]
    export_offset: i32,
    #[allow(dead_code)]
    depends_offset: i32,
    #[allow(dead_code)]
    guids_offset: i32,
}

#[derive(Clone, Debug, Default)]
pub struct WriteOptions {
    /// LZO-compress everything after the summary, like the cooker.
    pub compress: bool,
    /// XOR the compressed blocks with the Hi-Rez key (client packages).
    pub obscure: bool,
}

/// Split `Name_12` into (`Name`, 13); `Name` → (`Name`, 0). Mirrors
/// `FName::FName(const TCHAR*)` with `FNAME_Find/Add` splitting rules.
pub fn split_name_number(text: &str) -> (&str, u32) {
    if let Some(pos) = text.rfind('_') {
        let digits = &text[pos + 1..];
        if !digits.is_empty()
            && digits.bytes().all(|b| b.is_ascii_digit())
            && (digits == "0" || !digits.starts_with('0'))
            && digits.len() < 10
        {
            if let Ok(n) = digits.parse::<u32>() {
                return (&text[..pos], n + 1);
            }
        }
    }
    (text, 0)
}

// ----- compression ------------------------------------------------------

/// Read one `FCompressedChunkHeader` + blocks (`FArchive::SerializeCompressed`
/// layout: tag, block size, compressed size, uncompressed size, then
/// per-block (compressed, uncompressed) sizes, then the LZO blocks) at the
/// reader's position and return the expanded bytes. The same layout is used
/// for package chunks and for bulk data flagged `BULKDATA_SerializeCompressedLZO`.
pub fn read_compressed_chunk(r: &mut Reader, obscured: bool) -> Result<Vec<u8>> {
    let tag = r.u32()?;
    if tag != flags::PACKAGE_FILE_TAG && tag != flags::PACKAGE_FILE_TAG_SWAPPED {
        return Err(Error::BadTag(tag));
    }
    let block_size = r.u32()? as usize;
    let _total_comp = r.u32()? as usize;
    let total_uncomp = r.u32()? as usize;
    let nblocks = total_uncomp.div_ceil(block_size.max(1));
    let mut blocks = Vec::with_capacity(nblocks);
    for _ in 0..nblocks {
        blocks.push((r.u32()? as usize, r.u32()? as usize));
    }
    let mut out = Vec::with_capacity(total_uncomp);
    for (csize, usize_) in blocks {
        let comp = r.take(csize)?;
        let comp: std::borrow::Cow<[u8]> = if obscured {
            comp.iter().map(|b| b ^ compression::OBSCURE_KEY).collect::<Vec<u8>>().into()
        } else {
            comp.into()
        };
        let dec = lzo::decompress(&comp, usize_)?;
        out.extend_from_slice(&dec);
    }
    Ok(out)
}

/// Inverse of [`read_compressed_chunk`]: one chunk header plus LZO blocks of
/// `LOADING_COMPRESSION_CHUNK_SIZE` bytes, optionally XOR-obscured.
pub fn write_compressed_chunk(payload: &[u8], obscure: bool) -> Vec<u8> {
    let block_size = flags::LOADING_COMPRESSION_CHUNK_SIZE as usize;
    let blocks: Vec<Vec<u8>> = payload
        .chunks(block_size)
        .map(|b| {
            let mut c = lzo::compress(b);
            if obscure {
                for x in c.iter_mut() {
                    *x ^= compression::OBSCURE_KEY;
                }
            }
            c
        })
        .collect();
    let total_comp: usize = blocks.iter().map(|b| b.len()).sum();
    let mut w = Writer::new();
    w.u32(flags::PACKAGE_FILE_TAG);
    w.u32(block_size as u32);
    w.u32(total_comp as u32);
    w.u32(payload.len() as u32);
    for (b, src) in blocks.iter().zip(payload.chunks(block_size)) {
        w.u32(b.len() as u32);
        w.u32(src.len() as u32);
    }
    for b in &blocks {
        w.bytes(b);
    }
    w.into_inner()
}

/// Expand compressed chunks into a flat file image: the raw header bytes
/// (up to the first chunk) followed by each chunk at its uncompressed offset.
/// Chunks starting at or beyond `limit` are left out (and the image ends with
/// the last chunk kept), so a header-only read skips the content.
fn decompress_image(bytes: &[u8], summary: &Summary, limit: usize) -> Result<Vec<u8>> {
    let ctype = summary.compression_flags & compression::TYPE_MASK;
    if ctype != compression::LZO {
        return Err(Error::Compression(format!(
            "unsupported compression flags {:#x} (only LZO is implemented)",
            summary.compression_flags
        )));
    }
    let obscured = summary.compression_flags & compression::OBSCURED != 0;
    let chunks: Vec<&CompressedChunk> = summary.compressed_chunks.iter().filter(|c| (c.uncompressed_offset as usize) < limit).collect();
    let total = chunks.iter().map(|c| (c.uncompressed_offset + c.uncompressed_size) as usize).max().unwrap_or(0);
    let first = summary.compressed_chunks.iter().map(|c| c.uncompressed_offset as usize).min().unwrap_or(0);
    let mut image = vec![0u8; total.max(first)];
    let head = first.min(bytes.len());
    image[..head].copy_from_slice(&bytes[..head]);

    for chunk in chunks {
        let mut r = Reader::new(bytes);
        r.seek(chunk.compressed_offset as usize);
        let dec = read_compressed_chunk(&mut r, obscured)?;
        if dec.len() != chunk.uncompressed_size as usize {
            return Err(Error::Compression(format!(
                "chunk size mismatch: summary {} vs header {}",
                chunk.uncompressed_size,
                dec.len()
            )));
        }
        let out_pos = chunk.uncompressed_offset as usize;
        image[out_pos..out_pos + dec.len()].copy_from_slice(&dec);
    }
    Ok(image)
}

/// LZO every chunk; the chunks are independent, so they are spread over the
/// cores (compression is three quarters of a package rewrite otherwise).
fn compress_chunks(image: &[u8], ranges: &[(usize, usize)], obscure: bool) -> Vec<Vec<u8>> {
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(ranges.len()).max(1);
    if workers == 1 {
        return ranges.iter().map(|&(o, s)| write_compressed_chunk(&image[o..o + s], obscure)).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<Option<Vec<u8>>> = vec![None; ranges.len()];
    let slots: Vec<std::sync::Mutex<Option<Vec<u8>>>> = (0..ranges.len()).map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if i >= ranges.len() {
                    break;
                }
                let (o, s) = ranges[i];
                let c = write_compressed_chunk(&image[o..o + s], obscure);
                *slots[i].lock().unwrap() = Some(c);
            });
        }
    });
    for (dst, slot) in out.iter_mut().zip(slots) {
        *dst = slot.into_inner().unwrap();
    }
    out.into_iter().map(|c| c.unwrap()).collect()
}

/// Compress the file image the way the cooker does: the header tables form one chunk,
/// then whole export payloads are grouped into chunks of at most 1 MiB. Offsets in the
/// summary stay in uncompressed-image coordinates (the summary there has no chunk table).
fn compress_image(image: &[u8], summary: &mut Summary, obscure: bool, exports: &[(i32, i32)]) -> Vec<u8> {
    // `FArchiveSaveCompressed::CompressArchive` + `AddToChunk` (SavePackage.cpp):
    // the first chunk starts as the header (name table to TotalHeaderSize);
    // each export is appended to the current chunk unless that would exceed
    // MAX_MERGED_COMPRESSION_CHUNKSIZE, in which case the chunk is finished
    // and the export opens the next one (which may therefore exceed the limit).
    const MAX_MERGED_COMPRESSION_CHUNKSIZE: usize = 1024 * 1024;
    let start = summary.name_offset as usize;
    let header_end = summary.total_header_size as usize;
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut cur: (usize, usize) = (start, header_end - start);
    for &(_, size) in exports {
        let size = size.max(0) as usize;
        if cur.1 + size > MAX_MERGED_COMPRESSION_CHUNKSIZE {
            ranges.push(cur);
            cur = (cur.0 + cur.1, size);
        } else {
            cur.1 += size;
        }
    }
    ranges.push(cur);
    let comp_chunks = compress_chunks(image, &ranges, obscure);
    summary.compression_flags = compression::LZO | if obscure { compression::OBSCURED } else { 0 };
    summary.package_flags |= pkg::STORE_COMPRESSED;
    summary.compressed_chunks = vec![CompressedChunk::default(); ranges.len()];
    let mut probe = Writer::new();
    summary.write(&mut probe);
    let mut comp_off = probe.tell() as i32;
    for (i, (&(o, s), c)) in ranges.iter().zip(&comp_chunks).enumerate() {
        summary.compressed_chunks[i] = CompressedChunk { uncompressed_offset: o as i32, uncompressed_size: s as i32, compressed_offset: comp_off, compressed_size: c.len() as i32 };
        comp_off += c.len() as i32;
    }
    let mut w = Writer::new();
    summary.write(&mut w);
    for c in &comp_chunks {
        w.bytes(c);
    }
    w.into_inner()
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "version:            {}/{}", self.file_version, self.licensee_version)?;
        writeln!(f, "engine version:     {}", self.engine_version)?;
        writeln!(f, "cooked version:     {:#x}", self.cooked_content_version)?;
        writeln!(f, "folder:             {}", self.folder_name)?;
        writeln!(f, "package flags:      {:#010x}", self.package_flags)?;
        writeln!(f, "total header size:  {}", self.total_header_size)?;
        writeln!(f, "names:              {} @ {}", self.name_count, self.name_offset)?;
        writeln!(f, "imports:            {} @ {}", self.import_count, self.import_offset)?;
        writeln!(f, "exports:            {} @ {}", self.export_count, self.export_offset)?;
        writeln!(f, "depends:            @ {}", self.depends_offset)?;
        writeln!(f, "guids:              {}/{} @ {}", self.import_guids_count, self.export_guids_count, self.import_export_guids_offset)?;
        writeln!(f, "thumbnails:         @ {}", self.thumbnail_table_offset)?;
        writeln!(f, "guid:               {}", self.guid)?;
        writeln!(f, "compression flags:  {:#x}", self.compression_flags)?;
        writeln!(f, "compressed chunks:  {}", self.compressed_chunks.len())?;
        writeln!(f, "package source:     {:#x}", self.package_source)?;
        for g in &self.generations {
            writeln!(f, "generation:         exports={} names={} net={}", g.export_count, g.name_count, g.net_object_count)?;
        }
        Ok(())
    }
}
