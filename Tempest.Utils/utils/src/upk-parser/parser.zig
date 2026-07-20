const Parser = @This();

const std = @import("std");
const Io = std.Io;
const mem = std.mem;

const Tempest_Utils = @import("Tempest_Utils");
const minilzo = Tempest_Utils.minilzo;

const archive = @import("archive.zig");
const constants = @import("constants.zig");
const core = @import("core.zig");
const objects = @import("objects.zig");

allocator: std.mem.Allocator,
file_buffer: []u8,
summary: archive.FPackageFileSummary = .{},
names_table: []archive.FNameEntry = &.{},
imports_table: []archive.FObjectImport = &.{},
exports_table: []archive.FObjectExport = &.{},
depends_table: [][]u32 = &.{},
data_buffer: []u8 = &.{},
options: ParserOptions = .{},

pub const ParserOptions = struct {
    verbose: bool = false,
};

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
    parser.allocator.free(parser.names_table);
    parser.allocator.free(parser.imports_table);
    parser.allocator.free(parser.exports_table);
    for (parser.depends_table) |d| parser.allocator.free(d);
    parser.allocator.free(parser.depends_table);
    parser.allocator.free(parser.data_buffer);
    parser.* = undefined;
}

pub fn decompress(parser: *Parser, reader: *Io.Reader) !void {
    var decompressed_data: std.ArrayList([]u8) = .empty;
    errdefer decompressed_data.deinit(parser.allocator);

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
    }

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

    self.imports_table = try self.allocator.alloc(archive.FObjectImport, self.summary.import_count + 1);
    self.imports_table[0] = .{
        .class_package = 0,
        .class_name = 0,
        .outer_index = 0,
        .unk1 = 0,
        .owner_ref = 0,
        .object_name = 0,
        .unk2 = 0,
    };
    for (self.imports_table[1..]) |*import| {
        import.* = try archive.FObjectImport.take(reader);
    }

    self.exports_table = try self.allocator.alloc(archive.FObjectExport, self.summary.export_count + 1);
    self.exports_table[0] = .{
        .class_index = 0,
        .super_index = 0,
        .outer_index = 0,
        .object_name = 0,
        .archetype_index = 0,
        .archetype = 0,
        .object_flags = 0,
        .serial_size = 0,
        .serial_offset = 0,
        .export_flags = 0,
        .generation_net_object_count = 0,
        .package_guid = .{},
        .package_flags = 0,
    };
    for (self.exports_table[1..]) |*entry| {
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
