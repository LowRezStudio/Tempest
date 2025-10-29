const std = @import("std");
const mem = std.mem;
const fs = std.fs;

const Constants = @import("constants.zig");
const Minilzo = @import("../root.zig").minilzo;
const Archive = @import("archive.zig");

pub const Parser = struct {
    allocator: mem.Allocator = undefined,
    file_buffer: []u8 = undefined,
    summary: Archive.FPackageFileSummary = .{},
    names_table: []Archive.FNameEntry = &.{},
    imports_table: []Archive.FObjectImport = &.{},
    exports_table: []Archive.FObjectExport = &.{},
    depends_table: [][]u8 = &.{},
    rest_of_file: []u8 = &.{},

    pub fn init(allocator: mem.Allocator, file: fs.File) !Parser {
        const file_stats = try file.stat();
        const file_buffer = try file.readToEndAlloc(allocator, file_stats.size);

        return Parser{
            .allocator = allocator,
            .file_buffer = file_buffer,
        };
    }

    pub fn decompress(self: *Parser, reader: *std.Io.Reader) !void {
        var decompressed_data = std.ArrayList([]u8).empty;
        errdefer decompressed_data.deinit(self.allocator);

        for (self.summary.compressed_chunks) |chunk| {
            reader.seek = chunk.CompressedOffset;

            // Read chunk header
            const package_file_tag = try Archive.FCompressedChunkInfo.take(reader);
            const file_tag = package_file_tag.compressed_size;
            const loading_compression_chunk_size = package_file_tag.uncompressed_size;

            if (file_tag != Constants.file_tag and file_tag != Constants.swapped_file_tag) {
                return Archive.ArchiveError.InvalidPackageFileTag;
            }

            // Read chunk summary
            const summary = try Archive.FCompressedChunkInfo.take(reader);
            const blocks_count = (summary.uncompressed_size + loading_compression_chunk_size - 1) / loading_compression_chunk_size;

            // Read blocks header in chunk
            const blocks = try self.allocator.alloc(Archive.FCompressedChunkInfo, blocks_count);
            defer self.allocator.free(blocks);

            var max_compressed_size: u32 = 0;
            for (blocks) |*block| {
                block.* = try Archive.FCompressedChunkInfo.take(reader);
                max_compressed_size = @max(max_compressed_size, block.compressed_size);
            }

            const decompressed_chunk = try self.allocator.alloc(u8, summary.uncompressed_size);
            errdefer self.allocator.free(decompressed_chunk);

            var offset: usize = 0;

            // Read and decompress each block
            for (blocks, 0..) |block, block_idx| {
                // Read compressed block data
                var compressed_data = try self.allocator.alloc(u8, block.compressed_size);
                defer self.allocator.free(compressed_data);

                compressed_data = try reader.readAlloc(self.allocator, compressed_data.len);

                // Decompress block
                const decompressed_block = try Minilzo.decompressMemory(
                    self.allocator,
                    compressed_data,
                    block.uncompressed_size,
                    self.summary.compression_flags.obscured,
                );
                defer self.allocator.free(decompressed_block);

                // Copy decompressed data into the chunk buffer
                @memcpy(decompressed_chunk[offset .. offset + decompressed_block.len], decompressed_block);
                offset += decompressed_block.len;

                std.debug.print("Decompressed block {d}: {d} -> {d} bytes\n", .{
                    block_idx,
                    block.compressed_size,
                    block.uncompressed_size,
                });
            }

            std.debug.print("Chunk fully decompressed: {d} bytes total\n", .{offset});
            try decompressed_data.append(self.allocator, decompressed_chunk);
        }

        // fix summary
        self.summary.compression_flags = .{};
        self.summary.package_flags.store_compressed = false;
        self.summary.compressed_chunks = &.{};

        // Calculate total decompressed size
        var decompressed_size: usize = 0;
        for (decompressed_data.items) |decompressed_chunk| {
            decompressed_size += decompressed_chunk.len;
        }

        // Create new buffer with header + decompressed data
        const new_buffer = try self.allocator.alloc(u8, self.summary.name_offset + decompressed_size);
        errdefer self.allocator.free(new_buffer);

        // Copy header (everything before name_offset)
        @memcpy(new_buffer[0..self.summary.name_offset], self.file_buffer[0..self.summary.name_offset]);

        // Copy all decompressed chunks after the header
        var offset: usize = self.summary.name_offset;
        for (decompressed_data.items) |decompressed_chunk| {
            @memcpy(new_buffer[offset..][0..decompressed_chunk.len], decompressed_chunk);
            offset += decompressed_chunk.len;
        }

        // Free old buffer and replace with new one
        self.allocator.free(self.file_buffer);
        self.file_buffer = new_buffer;
    }

    pub fn parse(self: *Parser) !void {
        var fr = std.Io.Reader.fixed(self.file_buffer);
        var reader = &fr;

        self.summary = try Archive.FPackageFileSummary.take(reader, self.allocator);
        std.debug.print("{f}", .{self.summary});

        if (self.summary.compressed_chunks.len > 0) {
            try self.decompress(reader);

            // Reset reader to point to new buffer
            fr = std.io.Reader.fixed(self.file_buffer);
            reader = &fr;

            // Seek to name_offset to continue reading
            reader.seek = self.summary.name_offset;
        }

        // Read the names table
        self.names_table = try self.allocator.alloc(Archive.FNameEntry, self.summary.name_count);

        for (self.names_table) |*name_entry| {
            name_entry.* = try Archive.FNameEntry.take(reader, self.allocator);
        }

        // debug print names table
        for (self.names_table) |name_entry| {
            std.debug.print("{f}\n", .{name_entry.name});
        }

        // Read the imports table
        self.imports_table = try self.allocator.alloc(Archive.FObjectImport, self.summary.import_count + 1);

        self.imports_table[0] = Archive.FObjectImport{
            .class_package = 0,
            .class_name = 0,
            .outer_index = 0,
            .unk1 = 0,
            .owner_ref = 0,
            .object_name = 0,
            .unk2 = 0,
        };
        for (self.imports_table[1..]) |*import| {
            import.* = try Archive.FObjectImport.take(reader);
        }

        // Read the exports table
        self.exports_table = try self.allocator.alloc(Archive.FObjectExport, self.summary.export_count + 1);

        self.exports_table[0] = Archive.FObjectExport{
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
            .package_guid = Archive.FGuid{
                .a = 0,
                .b = 0,
                .c = 0,
                .d = 0,
            },
            .package_flags = 0,
        };
        for (self.exports_table[1..]) |*@"export"| {
            @"export".* = try Archive.FObjectExport.take(reader, self.allocator);
        }

        // TODO: read the depends table

        // For now read the rest of the file for reconstruction later
        const file_size = self.file_buffer.len - reader.seek;
        self.rest_of_file = try reader.readAlloc(self.allocator, @intCast(file_size));
    }

    pub fn testWrite(self: *Parser, path: []const u8) !void {
        const file = try fs.cwd().createFile(path, .{});
        defer file.close();

        var buffer: [4096]u8 = undefined;
        var fw = file.writer(&buffer);
        const w = &fw.interface;

        // Write the summary
        try self.summary.write(w);

        // Write the names table
        for (self.names_table) |*name_entry| {
            try name_entry.write(w);
        }

        // Write the imports table (skip the first dummy entry)
        for (self.imports_table[1..]) |*import| {
            try import.write(w);
        }

        // Write the exports table (skip the first dummy entry)
        for (self.exports_table[1..]) |*@"export"| {
            try @"export".write(w);
        }

        // Write the rest of the file
        try w.writeAll(self.rest_of_file);

        // Flush the buffered writer
        try w.flush();
    }
};
