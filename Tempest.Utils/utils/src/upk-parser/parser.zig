const std = @import("std");
const Io = std.Io;
const mem = std.mem;

const Tempest_Utils = @import("Tempest_Utils");
const minilzo = Tempest_Utils.minilzo;

const archive = @import("archive.zig");
const constants = @import("constants.zig");
const core = @import("core.zig");
const objects = @import("objects.zig");

const Parser = @This();

pub const ParserOptions = struct {
    verbose: bool = false,
};

options: ParserOptions = .{},
allocator: std.mem.Allocator,
file_buffer: []u8,

summary: archive.FPackageFileSummary = .{},
names_table: []archive.FNameEntry = &.{},
imports_table: []archive.FObjectImport = &.{},
exports_table: []archive.FObjectExport = &.{},
depends_table: [][]u32 = &.{},
data_buffer: []u8 = &.{},

pub const ObjectRef = union(enum) {
    none,
    import: *const archive.FObjectImport,
    @"export": *const archive.FObjectExport,
};

pub fn resolveIndex(self: *const Parser, index: i32) ObjectRef {
    if (index == 0) return .none;
    if (index > 0) return .{ .@"export" = &self.exports_table[@intCast(index - 1)] };
    return .{ .import = &self.imports_table[@intCast(-index - 1)] };
}

pub fn resolveName(self: *const Parser, index: i32) ![]const u8 {
    if (index == 0) return "None";
    if (index > self.names_table.len) return error.BadIndex;

    return self.names_table[@intCast(index)].name.toString();
}

pub fn init(io: Io, allocator: mem.Allocator, filepath: []const u8, options: ParserOptions) !Parser {
    const file_buffer = try Io.Dir.readFileAlloc(Io.Dir.cwd(), io, filepath, allocator, .unlimited);

    return Parser{
        .allocator = allocator,
        .file_buffer = file_buffer,
        .options = options,
    };
}

pub fn deinit(parser: *Parser) void {
    parser.allocator.free(parser.file_buffer);
    parser.summary.deinit(parser.allocator);

    for (parser.names_table) |*entry| entry.deinit(parser.allocator);
    parser.allocator.free(parser.names_table);

    parser.allocator.free(parser.imports_table);

    for (parser.exports_table) |*entry| entry.deinit(parser.allocator);
    parser.allocator.free(parser.exports_table);

    for (parser.depends_table) |d| parser.allocator.free(d);
    parser.allocator.free(parser.depends_table);

    parser.allocator.free(parser.data_buffer);
    parser.* = undefined;
}

pub fn decompress(parser: *Parser, reader: *Io.Reader) !void {
    var decompressed_data: std.ArrayList([]u8) = .empty;

    errdefer {
        for (decompressed_data.items) |chunk| {
            parser.allocator.free(chunk);
        }
        decompressed_data.deinit(parser.allocator);
    }

    for (parser.summary.compressed_chunks) |chunk| {
        reader.seek = chunk.CompressedOffset;

        const package_file_tag = try archive.FCompressedChunkInfo.take(reader);
        const file_tag = package_file_tag.compressed_size;
        const loading_compression_chunk_size = package_file_tag.uncompressed_size;

        if (file_tag != constants.file_tag and file_tag != constants.swapped_file_tag) {
            return archive.ArchiveError.InvalidPackageFileTag;
        }

        const summary = try archive.FCompressedChunkInfo.take(reader);
        const blocks_count = (summary.uncompressed_size + loading_compression_chunk_size - 1) / loading_compression_chunk_size;

        const blocks = try parser.allocator.alloc(archive.FCompressedChunkInfo, blocks_count);
        defer parser.allocator.free(blocks);

        for (blocks) |*block| {
            block.* = try archive.FCompressedChunkInfo.take(reader);
        }

        const decompressed_chunk = try parser.allocator.alloc(u8, summary.uncompressed_size);
        errdefer parser.allocator.free(decompressed_chunk);

        var offset: usize = 0;
        for (blocks) |block| {
            const compressed_data = try reader.readAlloc(parser.allocator, block.compressed_size);
            defer parser.allocator.free(compressed_data);

            const decompressed_block = try minilzo.decompressMemory(
                parser.allocator,
                compressed_data,
                block.uncompressed_size,
                parser.summary.compression_flags.obscured,
            );
            defer parser.allocator.free(decompressed_block);

            @memcpy(decompressed_chunk[offset..][0..decompressed_block.len], decompressed_block);
            offset += decompressed_block.len;
        }

        try decompressed_data.append(parser.allocator, decompressed_chunk);
    }

    // NOTE: kept for the patcher, which needs to know a file *was* compressed
    const obscured = parser.summary.compression_flags.obscured;
    parser.summary.compression_flags = .{ .obscured = obscured };
    parser.summary.package_flags.store_compressed = false;
    parser.allocator.free(parser.summary.compressed_chunks);
    parser.summary.compressed_chunks = &.{};

    var decompressed_size: usize = 0;
    for (decompressed_data.items) |chunk| decompressed_size += chunk.len;

    const new_buffer = try parser.allocator.alloc(u8, parser.summary.name_offset + decompressed_size);
    errdefer parser.allocator.free(new_buffer);

    @memcpy(new_buffer[0..parser.summary.name_offset], parser.file_buffer[0..parser.summary.name_offset]);

    var offset: usize = parser.summary.name_offset;

    for (decompressed_data.items) |chunk| {
        @memcpy(new_buffer[offset..][0..chunk.len], chunk);
        offset += chunk.len;

        parser.allocator.free(chunk);
    }

    decompressed_data.deinit(parser.allocator);

    parser.allocator.free(parser.file_buffer);
    parser.file_buffer = new_buffer;
}

pub fn parse(self: *Parser) !void {
    var fr: Io.Reader = .fixed(self.file_buffer);
    const reader = &fr;

    self.summary = try archive.FPackageFileSummary.take(reader, self.allocator);
    if (self.options.verbose) std.debug.print("{f}", .{self.summary});

    if (self.summary.compressed_chunks.len > 0) {
        try self.decompress(reader);

        fr = .fixed(self.file_buffer);
        reader.seek = self.summary.name_offset;
    }

    self.names_table = try self.allocator.alloc(archive.FNameEntry, self.summary.name_count);
    for (self.names_table) |*name_entry| {
        name_entry.* = try archive.FNameEntry.take(reader, self.allocator, true);
    }

    self.imports_table = try self.allocator.alloc(archive.FObjectImport, self.summary.import_count);
    for (self.imports_table) |*import| {
        import.* = try archive.FObjectImport.take(reader, self.allocator);
    }

    self.exports_table = try self.allocator.alloc(archive.FObjectExport, self.summary.export_count);
    for (self.exports_table) |*entry| {
        entry.* = try archive.FObjectExport.take(reader, self.allocator);
    }

    const depends_count = try reader.takeInt(u32, .little);
    self.depends_table = try self.allocator.alloc([]u32, depends_count);
    for (self.depends_table) |*depends| {
        const count = try reader.takeInt(u32, .little);
        const depends_array = try self.allocator.alloc(u32, count);
        for (depends_array) |*d| d.* = try reader.takeInt(u32, .little);
        depends.* = depends_array;
    }

    const remaining = self.file_buffer.len - reader.seek;
    self.data_buffer = try reader.readAlloc(self.allocator, remaining);
}

pub fn save(self: *Parser, io: std.Io, filepath: []const u8) !void {
    const file = try Io.Dir.createFile(std.Io.Dir.cwd(), io, filepath, .{});
    defer file.close(io);

    var buffer: [4096]u8 = undefined;
    var writer = file.writer(io, &buffer);
    const w = &writer.interface;

    // Write the summary
    // NOTE: if obscured, and we still want to save, remove the flag
    self.summary.compression_flags.obscured = false;
    try self.summary.write(w);

    // Write the names table
    for (self.names_table) |*name_entry| {
        try name_entry.write(w, true);
    }

    // Write the imports table (skip the first dummy entry)
    for (self.imports_table) |*import| {
        try import.write(w);
    }

    // Write the exports table (skip the first dummy entry)
    for (self.exports_table) |*@"export"| {
        try @"export".write(w);
    }

    // Write the depends table
    try w.writeInt(u32, @intCast(self.depends_table.len), .little);
    for (self.depends_table) |depends| {
        try w.writeInt(u32, @intCast(depends.len), .little);
        for (depends) |d| {
            try w.writeInt(u32, d, .little);
        }
    }

    // Write the data
    try w.writeAll(self.data_buffer);
    try w.flush();
}
