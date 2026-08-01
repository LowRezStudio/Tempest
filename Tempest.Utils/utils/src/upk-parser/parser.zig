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

    // 3. Tables. The depends map and the thumbnail table are deliberately not
    //    parsed: both are skipped on cooked/standalone loads (§8 / §10 / §19.11).
    try self.parseNameMap();
    try self.parseImportMap();
    try self.parseExportMap();
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
