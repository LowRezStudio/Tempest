const std = @import("std");

const unreal = @import("unreal.zig");
const Compression = @import("compression.zig");

const FPackageFileSummary = unreal.FPackageFileSummary;
const FNameEntry = unreal.FNameEntry;
const FObjectImport = unreal.FObjectImport;
const FObjectExport = unreal.FObjectExport;
const FLevelGuids = unreal.FLevelGuids;
const ExportGuid = unreal.ExportGuid;

pub const Error = error{
    InvalidOffset,
} || std.mem.Allocator.Error;

const Parser = @This();

allocator: std.mem.Allocator,
file_buffer: []u8,
/// Uncompressed view of the whole package. Aliases `file_buffer` when the
/// package is stored plain; otherwise an owned decompressed image.
data_buffer: []u8,
data_owned: bool,

package_file_summary: FPackageFileSummary,
summary_parsed: bool,

name_map: []FNameEntry,
import_map: []FObjectImport,
export_map: []FObjectExport,
import_guids: []FLevelGuids,
export_guids: []ExportGuid,
/// Raw depends-map region bytes (between the export map and the GUID maps).
/// Cooked loads skip it (§8 / §19.11) but it is still physically on disk and
/// required by the game, so it is preserved verbatim. Aliases `data_buffer`.
depends_bytes: []const u8,
/// Slice per export into `data_buffer` (raw serialized object bytes).
export_data: [][]const u8,

pub fn init(io: std.Io, filepath: []const u8, allocator: std.mem.Allocator) !Parser {
    const file_buffer = try std.Io.Dir.readFileAlloc(std.Io.Dir.cwd(), io, filepath, allocator, .unlimited);

    return Parser{
        .allocator = allocator,
        .file_buffer = file_buffer,
        .data_buffer = file_buffer,
        .data_owned = false,
        .package_file_summary = undefined,
        .summary_parsed = false,
        .name_map = &.{},
        .import_map = &.{},
        .export_map = &.{},
        .import_guids = &.{},
        .export_guids = &.{},
        .depends_bytes = &.{},
        .export_data = &.{},
    };
}

pub fn deinit(self: *Parser) void {
    const a = self.allocator;

    a.free(self.file_buffer);
    if (self.data_owned) a.free(self.data_buffer);

    if (self.summary_parsed) self.package_file_summary.deinit(a);

    for (self.name_map) |*entry| entry.deinit(a);
    if (self.name_map.len > 0) a.free(self.name_map);

    if (self.import_map.len > 0) a.free(self.import_map);

    for (self.export_map) |*exp| exp.deinit(a);
    if (self.export_map.len > 0) a.free(self.export_map);

    for (self.import_guids) |*lg| lg.deinit(a);
    if (self.import_guids.len > 0) a.free(self.import_guids);

    if (self.export_guids.len > 0) a.free(self.export_guids);

    if (self.export_data.len > 0) a.free(self.export_data);
}

pub fn parse(self: *Parser) !void {
    // 1. Header (always plain, at file offset 0).
    var r: std.Io.Reader = .fixed(self.file_buffer);
    self.package_file_summary = try FPackageFileSummary.take(&r, self.allocator);
    self.summary_parsed = true;

    const summary = &self.package_file_summary;

    // 2. Rebuild the uncompressed image when the package is stored compressed
    //    (PARSING.md §20.3 / §20.5).
    if (summary.package_flags.store_compressed) {
        self.data_buffer = try Compression.decompressPackage(
            self.allocator,
            self.file_buffer,
            summary.compressed_chunks,
            @bitCast(summary.compression_flags),
        );
        self.data_owned = true;
    } else if (summary.package_flags.store_fully_compressed) {
        self.data_buffer = try Compression.decompressStream(
            self.allocator,
            self.file_buffer,
            @bitCast(summary.compression_flags),
        );
        self.data_owned = true;
    } else {
        self.data_buffer = self.file_buffer;
    }

    // 3. Tables. The depends map is skipped on cooked loads (§8 / §19.11) but
    //    the raw bytes are still on disk and the game needs them, so the span
    //    is captured (not interpreted) and re-emitted on save. The thumbnail
    //    table is stripped on this build and not parsed (§10 / §19.11).
    try self.parseNameMap();
    try self.parseImportMap();
    try self.parseExportMap();
    try self.parseDependsRegion();
    try self.parseGuidMaps();
    try self.extractExportData();
}

fn parseNameMap(self: *Parser) !void {
    const summary = &self.package_file_summary;
    if (summary.name_count <= 0) return;
    const a = self.allocator;

    const offset: usize = @intCast(summary.name_offset);
    if (offset >= self.data_buffer.len) return error.InvalidOffset;
    var r: std.Io.Reader = .fixed(self.data_buffer[offset..]);

    const map = try a.alloc(FNameEntry, @intCast(summary.name_count));
    var parsed: usize = 0;
    errdefer {
        for (map[0..parsed]) |*entry| entry.deinit(a);
        a.free(map);
    }
    for (map) |*entry| {
        entry.* = try FNameEntry.take(&r, a);
        parsed += 1;
    }
    self.name_map = map;
}

fn parseImportMap(self: *Parser) !void {
    const summary = &self.package_file_summary;
    if (summary.import_count <= 0) return;
    const a = self.allocator;

    const offset: usize = @intCast(summary.import_offset);
    if (offset >= self.data_buffer.len) return error.InvalidOffset;
    var r: std.Io.Reader = .fixed(self.data_buffer[offset..]);

    const map = try a.alloc(FObjectImport, @intCast(summary.import_count));
    errdefer a.free(map);
    for (map) |*imp| {
        imp.* = try FObjectImport.take(&r, a);
    }
    self.import_map = map;
}

fn parseExportMap(self: *Parser) !void {
    const summary = &self.package_file_summary;
    if (summary.export_count <= 0) return;
    const a = self.allocator;

    const offset: usize = @intCast(summary.export_offset);
    if (offset >= self.data_buffer.len) return error.InvalidOffset;
    var r: std.Io.Reader = .fixed(self.data_buffer[offset..]);

    const map = try a.alloc(FObjectExport, @intCast(summary.export_count));
    var parsed: usize = 0;
    errdefer {
        for (map[0..parsed]) |*exp| exp.deinit(a);
        a.free(map);
    }
    for (map) |*exp| {
        exp.* = try FObjectExport.take(&r, a);
        parsed += 1;
    }
    self.export_map = map;
}

/// Capture the raw depends-map byte span: `DependsOffset` up to the next
/// header region — the GUID maps, the thumbnail table, or the export data
/// (PARSING.md §1). Cooked packages still carry these bytes on disk (the
/// loader merely skips *interpreting* them, §8 / §19.11), so `save` can
/// re-emit them verbatim and stay byte-identical.
fn parseDependsRegion(self: *Parser) !void {
    const summary = &self.package_file_summary;
    if (summary.depends_offset <= 0) return;
    const start: usize = @intCast(summary.depends_offset);
    if (start >= self.data_buffer.len) return error.InvalidOffset;

    var end: ?usize = null;
    const successors = [_]i32{
        summary.import_export_guids_offset, // -1 (INDEX_NONE) when absent
        summary.thumbnail_table_offset, // 0 when absent
        summary.total_header_size,
    };
    for (successors) |next| {
        if (next > summary.depends_offset) {
            const e: usize = @intCast(next);
            if (end == null or e < end.?) end = e;
        }
    }
    // No region follows the depends map — nothing valid to preserve.
    const end_final = end orelse return;
    if (end_final > self.data_buffer.len) return error.InvalidOffset;

    self.depends_bytes = self.data_buffer[start..end_final];
}

fn parseGuidMaps(self: *Parser) !void {
    const summary = &self.package_file_summary;
    if (summary.import_export_guids_offset < 0) return;
    if (summary.import_guids_count <= 0 and summary.export_guids_count <= 0) return;
    const a = self.allocator;

    const offset: usize = @intCast(summary.import_export_guids_offset);
    if (offset >= self.data_buffer.len) return error.InvalidOffset;
    var r: std.Io.Reader = .fixed(self.data_buffer[offset..]);

    if (summary.import_guids_count > 0) {
        const guids = try a.alloc(FLevelGuids, @intCast(summary.import_guids_count));
        var parsed: usize = 0;
        errdefer {
            for (guids[0..parsed]) |*lg| lg.deinit(a);
            a.free(guids);
        }
        for (guids) |*lg| {
            lg.* = try FLevelGuids.take(&r, a);
            parsed += 1;
        }
        self.import_guids = guids;
    }

    if (summary.export_guids_count > 0) {
        const guids = try a.alloc(ExportGuid, @intCast(summary.export_guids_count));
        errdefer a.free(guids);
        for (guids) |*eg| {
            eg.* = try ExportGuid.take(&r, a);
        }
        self.export_guids = guids;
    }
}

fn extractExportData(self: *Parser) !void {
    if (self.export_map.len == 0) return;
    const a = self.allocator;

    const data = try a.alloc([]const u8, self.export_map.len);
    errdefer a.free(data);

    for (self.export_map, data) |exp, *blob| {
        if (exp.serial_size <= 0 or exp.serial_offset < 0) {
            blob.* = &.{};
            continue;
        }
        const off: usize = @intCast(exp.serial_offset);
        const size: usize = @intCast(exp.serial_size);
        if (off >= self.data_buffer.len) {
            // Tolerate a dangling pointer by clamping; keep the parse alive.
            blob.* = &.{};
            continue;
        }
        const end = @min(off + size, self.data_buffer.len);
        blob.* = self.data_buffer[off..end];
    }

    self.export_data = data;
}

// ---- Save / rebuild ---------------------------------------------------------

/// How `save` should write the rebuilt package back to disk.
pub const SaveMode = enum {
    compressed,
    uncompressed,
};

const SaveError = unreal.WriteError || Compression.Error || error{
    InvalidOffset,
};

/// Chunk size used by FFileCompressionHelper::CompressArchive for package-level
/// compression (PARSING.md §20.4): each chunk is ≤ 1 MiB uncompressed.
const package_chunk_size: usize = 0x100000;

/// Rebuild the package from the parsed structs and return the serialized file,
/// owned by `self.allocator` (free with the same allocator).
///
/// `.uncompressed` writes a plain package (compression bits cleared).
/// `.compressed` writes a PKG_StoreCompressed package: plain summary + maps,
/// with the export-data region split into `SerializeCompressed` chunk streams
/// (PARSING.md §20.3 / §20.4), matching what the Paladins loader reads.
///
/// This is the "edit via structs" workflow: modify `name_map` / `import_map` /
/// `export_map` / `export_data[i]` (keeping `export_map` and `export_data`
/// equal length) and save.
pub fn save(self: *Parser, mode: SaveMode) SaveError![]u8 {
    const a = self.allocator;
    const original = &self.package_file_summary;
    const epic_version = original.getFileVersion().version;

    // 1. Serialize the maps once (offset-independent), keeping the bytes.
    var name_buf: std.Io.Writer.Allocating = .init(a);
    defer name_buf.deinit();
    try self.writeNameMap(&name_buf.writer, a);
    const name_bytes = try name_buf.toOwnedSlice();
    defer a.free(name_bytes);

    var import_buf: std.Io.Writer.Allocating = .init(a);
    defer import_buf.deinit();
    try self.writeImportMap(&import_buf.writer, a);
    const import_bytes = try import_buf.toOwnedSlice();
    defer a.free(import_bytes);

    var export_measure: std.Io.Writer.Allocating = .init(a);
    defer export_measure.deinit();
    try self.writeExportMap(&export_measure.writer, a, self.export_map);
    const export_size = export_measure.writer.end;

    var guid_buf: std.Io.Writer.Allocating = .init(a);
    defer guid_buf.deinit();
    try self.writeGuidMaps(&guid_buf.writer, a);
    const guid_bytes = try guid_buf.toOwnedSlice();
    defer a.free(guid_bytes);

    // Size of the maps region [name_offset, total_header_size) in the rebuilt
    // uncompressed image: name map ++ import map ++ export map ++ depends map
    // (preserved verbatim) ++ GUID maps.
    const maps_len = name_bytes.len + import_bytes.len + export_size + self.depends_bytes.len + guid_bytes.len;

    // 2. Compressed chunk count. In a compressed package the maps region forms
    //    chunk 0 — the loader serves every read past the summary through the
    //    chunk map (FArchiveAsync::Precache → FindCompressedChunkIndex), so the
    //    name/import/export maps must be covered by a chunk or the load asserts.
    //    The export data is then chunked at export boundaries (§20.3): a chunk
    //    covers whole exports so no read spans two chunks. A read (an export's
    //    serial data) must fit entirely in one chunk because FArchiveAsync's
    //    Precache only serves a request contained in the current buffer — a
    //    request spanning two chunks busy-waits forever (a hang). Uniform 1 MiB
    //    cuts split exports across chunks; the original cooked packages cut at
    //    export boundaries, which `exportChunkEnds` reproduces exactly.
    const chunk_ends: []usize = if (mode == .compressed) try self.exportChunkEnds() else &.{};
    defer if (chunk_ends.len > 0) a.free(chunk_ends);
    const export_chunks: usize = chunk_ends.len;
    const chunk_count: usize = if (mode == .compressed) 1 + export_chunks else 0;

    // 3. Measure the summary sizes. Offsets never change the serialized length,
    //    so a placeholder summary with the final chunk-table count is enough.
    //
    //    Two sizes matter (PARSING.md §20.3):
    //      * `summary_size_plain` — the summary serialized with an EMPTY chunk
    //        table. The header offsets (NameOffset/.../TotalHeaderSize) describe
    //        the DECOMPRESSED layout and are anchored here; they must equal the
    //        values a plain (uncompressed) save would produce.
    //      * `summary_size` — the summary as written ON DISK for a compressed
    //        package: the plain fields plus the chunk table (4-byte count +
    //        16B × chunk_count). The compressed chunk streams begin at this
    //        offset in the file.
    var summary_copy = original.*;
    summary_copy.name_count = @intCast(self.name_map.len);
    summary_copy.import_count = @intCast(self.import_map.len);
    summary_copy.export_count = @intCast(self.export_map.len);
    summary_copy.import_guids_count = @intCast(self.import_guids.len);
    summary_copy.export_guids_count = @intCast(self.export_guids.len);
    summary_copy.total_header_size = 0;
    summary_copy.name_offset = 0;
    summary_copy.import_offset = 0;
    summary_copy.export_offset = 0;
    summary_copy.depends_offset = 0;
    summary_copy.import_export_guids_offset = 0;
    summary_copy.thumbnail_table_offset = 0;
    summary_copy.compression_flags = .{};
    // Placeholder entries: only the array count affects the serialized size.
    const placeholder_chunks = try a.alloc(unreal.FCompressedChunk, chunk_count);
    defer a.free(placeholder_chunks);
    summary_copy.compressed_chunks = placeholder_chunks;

    var size_buf: std.Io.Writer.Allocating = .init(a);
    defer size_buf.deinit();
    try summary_copy.write(&size_buf.writer, a);
    const summary_size = size_buf.writer.end;

    // Serialize once more with an empty chunk table for `summary_size_plain`.
    summary_copy.compressed_chunks = &.{};
    var plain_buf: std.Io.Writer.Allocating = .init(a);
    defer plain_buf.deinit();
    try summary_copy.write(&plain_buf.writer, a);
    const summary_size_plain = plain_buf.writer.end;

    // 4. Layout offsets (see PARSING.md §1): summary, name map, import map,
    //    export map, depends map (preserved verbatim), GUID maps, export data.
    //    These are offsets into the DECOMPRESSED image, so they anchor at the
    //    plain summary size, not the chunk-table-inflated on-disk size.
    const name_offset = summary_size_plain;
    const import_offset = name_offset + name_bytes.len;
    const export_offset = import_offset + import_bytes.len;
    const depends_offset = export_offset + export_size;
    const has_guid_region = epic_version > unreal.ver_guid_maps;
    const guid_offset = depends_offset + self.depends_bytes.len;
    const total_header_size = if (has_guid_region) guid_offset + guid_bytes.len else guid_offset;

    // 5. Exports with serial offsets pointing into the rebuilt layout.
    const exports = try self.buildExportsForSave(total_header_size);
    defer a.free(exports);

    // Rebuilt export-map bytes (offsets recorded in the package must match the
    // chunked layout; serial offsets are fixed-size fields so this is exactly
    // `export_size` bytes).
    var export_buf: std.Io.Writer.Allocating = .init(a);
    defer export_buf.deinit();
    try self.writeExportMap(&export_buf.writer, a, exports);
    const export_bytes = try export_buf.toOwnedSlice();
    defer a.free(export_bytes);

    // 6. Export-data region: concatenated, or split into chunk streams.
    const export_data_concat = try self.concatExportData(a);
    defer a.free(export_data_concat);

    const chunk_table = try a.alloc(unreal.FCompressedChunk, chunk_count);
    defer a.free(chunk_table);
    var streams_buf: std.Io.Writer.Allocating = .init(a);
    defer streams_buf.deinit();
    if (mode == .compressed) {
        const flags = self.compressionFlagsForSave();
        var chunk_idx: usize = 0;
        // Compressed chunk streams begin right after the summary ON DISK. The
        // on-disk summary appends the chunk table past `name_offset`, so the
        // streams start at `summary_size`, not `name_offset` (which describes
        // the decompressed layout).
        var compressed_cursor: usize = summary_size;

        // Chunk 0 = the maps region [name_offset, total_header_size). Without a
        // covering chunk, FArchiveAsync::Precache → FindCompressedChunkIndex
        // returns ArrayNum and PrecacheCompressedChunk asserts on the maps read.
        const maps_buf = try a.alloc(u8, maps_len);
        defer a.free(maps_buf);
        var moff: usize = 0;
        for ([_][]const u8{ name_bytes, import_bytes, export_bytes, self.depends_bytes, guid_bytes }) |part| {
            @memcpy(maps_buf[moff .. moff + part.len], part);
            moff += part.len;
        }
        const maps_stream = try Compression.compressStream(a, flags, maps_buf);
        defer a.free(maps_stream);
        chunk_table[0] = .{
            .uncompressed_offset = @intCast(name_offset),
            .uncompressed_size = @intCast(maps_len),
            .compressed_offset = @intCast(compressed_cursor),
            .compressed_size = @intCast(maps_stream.len),
        };
        try streams_buf.writer.writeAll(maps_stream);
        compressed_cursor += maps_stream.len;
        chunk_idx = 1;

        // Remaining chunks: export data, one chunk per whole-exports run
        // (`chunk_ends` holds the cumulative end of each export chunk into the
        // export-data region, so no export is split across chunks).
        var prev: usize = 0;
        for (chunk_ends) |end| {
            const us = end - prev;
            const stream = try Compression.compressStream(a, flags, export_data_concat[prev..end]);
            defer a.free(stream);
            chunk_table[chunk_idx] = .{
                .uncompressed_offset = @intCast(total_header_size + prev),
                .uncompressed_size = @intCast(us),
                .compressed_offset = @intCast(compressed_cursor),
                .compressed_size = @intCast(stream.len),
            };
            try streams_buf.writer.writeAll(stream);
            compressed_cursor += stream.len;
            prev = end;
            chunk_idx += 1;
        }
    }
    const streams = try streams_buf.toOwnedSlice();
    defer a.free(streams);

    // 7. Final summary with the computed offsets and mode-specific flags.
    var summary_final = original.*;
    summary_final.name_count = @intCast(self.name_map.len);
    summary_final.import_count = @intCast(self.import_map.len);
    summary_final.export_count = @intCast(self.export_map.len);
    summary_final.import_guids_count = @intCast(self.import_guids.len);
    summary_final.export_guids_count = @intCast(self.export_guids.len);
    summary_final.total_header_size = @intCast(total_header_size);
    summary_final.name_offset = @intCast(name_offset);
    summary_final.import_offset = @intCast(import_offset);
    summary_final.export_offset = @intCast(export_offset);
    summary_final.depends_offset = @intCast(depends_offset);
    summary_final.import_export_guids_offset = if (has_guid_region) @intCast(guid_offset) else -1;
    summary_final.thumbnail_table_offset = 0;
    summary_final.package_flags.store_compressed = mode == .compressed;
    summary_final.package_flags.store_fully_compressed = false;
    summary_final.compression_flags = if (mode == .compressed) self.compressionFlagsStruct() else .{};
    summary_final.compressed_chunks = chunk_table;

    // 8. Assemble: summary ++ maps ++ export data. For compressed mode the maps
    //    are not emitted plain — they live inside chunk 0's stream, and the
    //    summary offsets (NameOffset/ImportOffset/.../TotalHeaderSize) are
    //    uncompressed offsets resolved through the chunk map.
    var out_buf: std.Io.Writer.Allocating = .init(a);
    defer out_buf.deinit();
    const w = &out_buf.writer;
    try summary_final.write(w, a);
    if (mode == .compressed) {
        try w.writeAll(streams);
    } else {
        try w.writeAll(name_bytes);
        try w.writeAll(import_bytes);
        try w.writeAll(export_bytes);
        try w.writeAll(self.depends_bytes);
        try w.writeAll(guid_bytes);
        try w.writeAll(export_data_concat);
    }
    return out_buf.toOwnedSlice();
}

fn exportDataSize(self: *const Parser) usize {
    var total: usize = 0;
    for (self.export_data) |blob| total += blob.len;
    return total;
}

fn concatExportData(self: *const Parser, allocator: std.mem.Allocator) ![]u8 {
    const total = self.exportDataSize();
    const buf = try allocator.alloc(u8, total);
    var off: usize = 0;
    for (self.export_data) |blob| {
        @memcpy(buf[off .. off + blob.len], blob);
        off += blob.len;
    }
    return buf;
}

/// Cumulative end offset of each export-data chunk (last element equals the
/// export-data size). Each chunk spans whole exports, so an export's serial
/// data never crosses a chunk boundary. The game's `FArchiveAsync` serves a
/// read only from the current buffer; a read spanning two chunks can never be
/// satisfied and busy-waits forever (a hang), so the cut points must fall
/// between exports, never inside one. Chunks are ≤ `package_chunk_size`, except
/// a single export at/over the chunk size gets a chunk of its own. This mirrors
/// how the original cooked packages cut their export data.
fn exportChunkEnds(self: *const Parser) ![]usize {
    const a = self.allocator;

    // First pass: count the chunks.
    var count: usize = 0;
    var chunk_len: usize = 0; // length of the currently open chunk
    for (self.export_data) |blob| {
        if (blob.len == 0) continue;
        // Adding this blob would overflow the chunk cap and the chunk is
        // non-empty: close it, leaving the blob to start the next chunk.
        if (chunk_len > 0 and chunk_len + blob.len > package_chunk_size) {
            count += 1;
            chunk_len = 0;
        }
        chunk_len += blob.len;
        // A single export at/over the cap closes a chunk on its own, so it is
        // never split across two chunks.
        if (blob.len >= package_chunk_size) {
            count += 1;
            chunk_len = 0;
        }
    }
    if (chunk_len > 0) count += 1;

    // Second pass: fill the cumulative end offset of each chunk.
    const ends = try a.alloc(usize, count);
    var idx: usize = 0;
    var cursor: usize = 0; // end of the export data covered so far
    chunk_len = 0;
    for (self.export_data) |blob| {
        if (blob.len == 0) continue;
        if (chunk_len > 0 and chunk_len + blob.len > package_chunk_size) {
            cursor += chunk_len;
            ends[idx] = cursor;
            idx += 1;
            chunk_len = 0;
        }
        chunk_len += blob.len;
        if (blob.len >= package_chunk_size) {
            cursor += chunk_len;
            ends[idx] = cursor;
            idx += 1;
            chunk_len = 0;
        }
    }
    if (chunk_len > 0) {
        cursor += chunk_len;
        ends[idx] = cursor;
        idx += 1;
    }
    return ends;
}

/// A shallow copy of the export map with `serial_offset`/`serial_size` updated
/// to point into the rebuilt layout (export data starts at `total_header_size`
/// and runs contiguously in export order). Exports with no data keep the
/// original no-data representation (`serial_offset = 0, serial_size = 0`) —
/// the original files store them that way, and assigning a cursor offset would
/// produce a byte-different export table.
fn buildExportsForSave(self: *Parser, total_header_size: usize) ![]unreal.FObjectExport {
    const a = self.allocator;
    const exports = try a.alloc(unreal.FObjectExport, self.export_map.len);
    errdefer a.free(exports);
    var cursor: usize = total_header_size;
    for (self.export_map, self.export_data, exports) |*src, blob, *dst| {
        dst.* = src.*;
        if (blob.len == 0) {
            dst.serial_offset = 0;
            dst.serial_size = 0;
            continue;
        }
        dst.serial_offset = @intCast(cursor);
        dst.serial_size = @intCast(blob.len);
        cursor += blob.len;
    }
    return exports;
}

fn writeNameMap(self: *Parser, writer: *std.Io.Writer, allocator: std.mem.Allocator) SaveError!void {
    for (self.name_map) |*entry| {
        try entry.write(writer, allocator);
    }
}

fn writeImportMap(self: *Parser, writer: *std.Io.Writer, allocator: std.mem.Allocator) SaveError!void {
    for (self.import_map) |*imp| {
        try imp.write(writer, allocator);
    }
}

fn writeExportMap(
    self: *Parser,
    writer: *std.Io.Writer,
    allocator: std.mem.Allocator,
    exports: []const unreal.FObjectExport,
) SaveError!void {
    _ = self;
    for (exports) |*exp| {
        try exp.write(writer, allocator);
    }
}

fn writeGuidMaps(self: *Parser, writer: *std.Io.Writer, allocator: std.mem.Allocator) SaveError!void {
    for (self.import_guids) |*lg| {
        try lg.write(writer, allocator);
    }
    for (self.export_guids) |*eg| {
        try eg.write(writer, allocator);
    }
}

/// Compression flags for a compressed save: preserve the parsed codec/obscure
/// bits, defaulting the codec to LZO (Paladins' GBaseCompressionMethod) when
/// the source package was stored plain or with an unsupported codec.
fn compressionFlagsStruct(self: *const Parser) unreal.ECompressionFlags {
    var flags = self.package_file_summary.compression_flags;
    switch (flags.type) {
        .zlib, .lzo => {},
        .none, .lzx => flags.type = .lzo,
    }
    return flags;
}

fn compressionFlagsForSave(self: *const Parser) u32 {
    return @bitCast(self.compressionFlagsStruct());
}

// ---- Resolution helpers -----------------------------------------------------

/// Look up a name-map index, returning the base string (or a placeholder for
/// out-of-range indices).
pub fn resolveName(self: *const Parser, name_index: i32) []const u8 {
    if (name_index < 0 or name_index >= @as(i32, @intCast(self.name_map.len))) return "<bad name>";
    return self.name_map[@intCast(name_index)].name;
}

/// Format an FName, appending the instance number (`number - 1`) when nonzero.
pub fn formatFName(self: *const Parser, name: unreal.FName, buf: []u8) []const u8 {
    const base = self.resolveName(name.name_index);
    if (name.number == 0) return base;
    return std.fmt.bufPrint(buf, "{s}_{d}", .{ base, name.number - 1 }) catch base;
}

/// Resolve a PACKAGE_INDEX: >0 → export (index-1), <0 → import (-index-1),
/// 0 → the package itself (ROOTPACKAGE_INDEX).
pub fn resolvePackageIndex(self: *const Parser, index: i32, buf: []u8) []const u8 {
    if (index == 0) return "Package";
    if (index > 0) {
        const idx: usize = @intCast(index - 1);
        if (idx < self.export_map.len) {
            return self.formatFName(self.export_map[idx].object_name, buf);
        }
        return "<bad export>";
    }
    const idx: usize = @intCast(-index - 1);
    if (idx < self.import_map.len) {
        return self.formatFName(self.import_map[idx].object_name, buf);
    }
    return "<bad import>";
}

/// Resolve an export's ClassIndex: 0 means the object is a UClass itself
/// (PARSING.md §7), otherwise a normal PACKAGE_INDEX.
pub fn resolveClassIndex(self: *const Parser, index: i32, buf: []u8) []const u8 {
    if (index == 0) return "Class";
    return self.resolvePackageIndex(index, buf);
}
