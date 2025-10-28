const std = @import("std");
const mem = std.mem;
const fs = std.fs;

const minilzo = @import("utils").minilzo;

const ue = @import("ue.zig");

pub const Parser = struct {
    file: fs.File,
    allocator: mem.Allocator,
    summary: ue.FPackageFileSummary = undefined,
    names_table: []ue.FNameEntry = undefined,
    imports_table: []ue.FObjectImport = undefined,
    exports_table: []ue.FObjectExport = undefined,
    depends_table: [][]u8 = undefined,
    rest_of_file: []u8 = undefined,

    pub fn init(allocator: mem.Allocator, file: fs.File) !Parser {
        return Parser{
            .file = file,
            .allocator = allocator,
        };
    }

    pub fn decompress(self: *Parser, r: *std.Io.Reader) !void {
        try minilzo.init();

        _ = self;
        _ = r;

        // for (self.summary.compressed_chunks) |chunk| {
        //     const decompressed_size = chunk.UncompressedSize;
        //
        //     try self.file.seekTo(chunk.UncompressedOffset);
        //     const data = try r.readAlloc(self.allocator, decompressed_size);
        //     defer self.allocator.free(data);
        //
        //     const decompressed_data = try minilzo.decompress(self.allocator, data, decompressed_size);
        //
        //     var fw = try fs.cwd().createFile("test.bin", .{});
        //     defer fw.close();
        //     try fw.writeAll(decompressed_data);
        // }
    }

    pub fn parse(self: *Parser) !void {
        var buffer: [4096]u8 = undefined;
        var fr = self.file.reader(&buffer);
        const r: *std.Io.Reader = &fr.interface;

        self.summary = try ue.FPackageFileSummary.read(r, self.allocator);
        self.summary.debugPrint();

        // TODO: implement [de]compression
        if (self.summary.compressed_chunks.len > 0) {
            try self.decompress(r);
        }

        // Read the names table
        self.names_table = try self.allocator.alloc(ue.FNameEntry, self.summary.name_count);

        for (self.names_table) |*name_entry| {
            name_entry.* = try ue.FNameEntry.read(r, self.allocator);
        }

        // Read the imports table
        self.imports_table = try self.allocator.alloc(ue.FObjectImport, self.summary.import_count + 1);

        self.imports_table[0] = ue.FObjectImport{};
        for (self.imports_table[1..]) |*import| {
            import.* = try ue.FObjectImport.read(r);
        }

        // Read the exports table
        self.exports_table = try self.allocator.alloc(ue.FObjectExport, self.summary.export_count + 1);

        self.exports_table[0] = ue.FObjectExport{};
        for (self.exports_table[1..]) |*@"export"| {
            @"export".* = try ue.FObjectExport.read(r, self.allocator);
        }

        // TODO: read the depends table

        // For now read the rest of the file for reconstruction later
        const file_stat = try self.file.stat();
        const file_size = file_stat.size - fr.pos;
        self.rest_of_file = try r.readAlloc(self.allocator, @intCast(file_size));
    }

    pub fn deinit(self: *Parser) void {
        // Free the generations and folder name
        self.allocator.free(self.summary.generations);
        self.summary.folder_name.deinit(self.allocator);
        for (self.summary.additional_packages) |pkg| {
            pkg.deinit(self.allocator);
        }

        // Free additional packages
        self.allocator.free(self.summary.additional_packages);

        // Free compressed chunks
        self.allocator.free(self.summary.compressed_chunks);

        // Free texture allocations
        for (self.summary.texture_allocations) |allocation| {
            if (allocation.export_indices_count > 0) {
                self.allocator.free(allocation.export_indices[0..allocation.export_indices_count]);
            }
        }
        self.allocator.free(self.summary.texture_allocations);

        // Free all the names from the names table
        for (self.names_table) |name_entry| {
            name_entry.name.deinit(self.allocator);
        }
        self.allocator.free(self.names_table);

        // Free the imports table
        self.allocator.free(self.imports_table);

        // Free the exports table
        for (self.exports_table[1..]) |@"export"| {
            if (@"export".generation_net_object_count > 0) {
                self.allocator.free(@"export".generation_net_objects[0..@"export".generation_net_object_count]);
            }
        }
        self.allocator.free(self.exports_table);

        // Free the rest
        // self.allocator.free(self.depends_table);
        self.allocator.free(self.rest_of_file);
    }
};
