const std = @import("std");
const mem = std.mem;
const fs = std.fs;

const minilzo = @import("../root.zig").minilzo;
const archive = @import("archive.zig");

pub const Parser = struct {
    file: fs.File,
    allocator: mem.Allocator,
    summary: archive.FPackageFileSummary = undefined,
    names_table: []archive.FNameEntry = undefined,
    imports_table: []archive.FObjectImport = undefined,
    exports_table: []archive.FObjectExport = undefined,
    depends_table: [][]u8 = undefined,
    rest_of_file: []u8 = undefined,

    pub fn init(allocator: mem.Allocator, file: fs.File) !Parser {
        return Parser{
            .file = file,
            .allocator = allocator,
        };
    }

    pub fn parse(self: *Parser) !void {
        var buffer: [4096]u8 = undefined;
        var fr = self.file.reader(&buffer);
        const r: *std.Io.Reader = &fr.interface;

        self.summary = try archive.FPackageFileSummary.take(r, self.allocator);
        std.debug.print("{f}\n", .{self.summary});

        // TODO: implement [de]compression
        // if (self.summary.compressed_chunks.len > 0) {
        //     try self.decompress(r);
        // }

        // Read the names table
        self.names_table = try self.allocator.alloc(archive.FNameEntry, self.summary.name_count);

        for (self.names_table) |*name_entry| {
            name_entry.* = try archive.FNameEntry.take(r, self.allocator);
        }

        // Read the imports table
        self.imports_table = try self.allocator.alloc(archive.FObjectImport, self.summary.import_count + 1);

        self.imports_table[0] = archive.FObjectImport{
            .class_package = 0,
            .class_name = 0,
            .outer_index = 0,
            .unk1 = 0,
            .owner_ref = 0,
            .object_name = 0,
            .unk2 = 0,
        };
        for (self.imports_table[1..]) |*import| {
            import.* = try archive.FObjectImport.take(r);
        }

        // Read the exports table
        self.exports_table = try self.allocator.alloc(archive.FObjectExport, self.summary.export_count + 1);

        self.exports_table[0] = archive.FObjectExport{
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
            .package_guid = archive.FGuid{
                .a = 0,
                .b = 0,
                .c = 0,
                .d = 0,
            },
            .package_flags = 0,
        };
        for (self.exports_table[1..]) |*@"export"| {
            @"export".* = try archive.FObjectExport.take(r, self.allocator);
        }

        // TODO: read the depends table

        // For now read the rest of the file for reconstruction later
        const file_stat = try self.file.stat();
        const file_size = file_stat.size - fr.logicalPos();

        self.rest_of_file = try r.readAlloc(self.allocator, @intCast(file_size));
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
