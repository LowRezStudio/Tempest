const std = @import("std");
const mem = std.mem;
const fs = std.fs;

const ue = @import("ue.zig");

const Parser = struct {
    file: fs.File,
    allocator: mem.Allocator,
    summary: ue.FPackageFileSummary = undefined,
    names_table: []ue.FNameEntry = undefined,
    imports_table: []ue.FObjectImport = undefined,
    exports_table: []ue.FObjectExport = undefined,
    depends_table: [][]u8 = undefined,

    pub fn init(allocator: mem.Allocator, file: fs.File) !Parser {
        return Parser{
            .file = file,
            .allocator = allocator,
        };
    }

    pub fn patchSFSC(self: *Parser, filepath: []const u8) !void {
        const SFSCExport = struct {
            object: ue.FObjectExport,
            index: usize,
        };

        var sfsc_exports = std.ArrayList(SFSCExport).empty;
        defer sfsc_exports.deinit(self.allocator);

        for (self.exports_table, 0..) |@"export", i| {
            if (std.mem.eql(u8, self.names_table[@"export".NameTableIndex].name.toString(), "SeekFreeShaderCache\x00")) {
                std.debug.print("Found SeekFreeShaderCache export at index {d}\n", .{i});
                try sfsc_exports.append(self.allocator, .{
                    .object = @"export",
                    .index = i,
                });
            }
        }

        std.debug.print("SFSC Exports:\n", .{});
        for (sfsc_exports.items) |@"export"| {
            const obj_type_name = if (@"export".object.ClassIndex < 0)
                self.names_table[self.imports_table[@abs(@"export".object.ClassIndex)].NameTableIndex].name.toString()
            else
                self.names_table[@abs(@"export".object.ClassIndex)].name.toString();

            std.debug.print(
                \\
                \\  Name:          {s}_{d}
                \\  UniqueId:      {d}
                \\  SerialSize:    {d}
                \\  Type:          {s}
                \\  Parent:        {d} ({s}_{d})
                \\  Owner:         {d} ({s}_{d})
                \\
                \\
            , .{
                self.names_table[@abs(@"export".object.NameTableIndex)].name.toString(),
                @"export".object.NameCount,
                @"export".object.NameCount,
                @"export".object.SerialSize,
                obj_type_name,
                @"export".object.SuperIndex,
                self.names_table[self.exports_table[@abs(@"export".object.SuperIndex)].NameTableIndex].name.toString(),
                self.exports_table[@abs(@"export".object.SuperIndex)].NameCount,
                @"export".object.ArchetypeIndex,
                self.names_table[self.exports_table[@abs(@"export".object.ArchetypeIndex)].NameTableIndex].name.toString(),
                self.exports_table[@abs(@"export".object.ArchetypeIndex)].NameCount,
            });

            // Patch the SFSC export
            // var name_index: u32 = 0;
            // for (self.names_table, 0..) |name, i| {
            //     if (mem.eql(u8, name.name.toString(), "Package\x00")) {
            //         name_index = @intCast(i);
            //         break;
            //     }
            // }
            //

            // Find the object with name "Package\x00" in the import table
            var obj_index: i32 = 0;
            for (self.imports_table[1..], 1..) |import, i| {
                if (mem.eql(u8, self.names_table[import.NameTableIndex].name.toString(), "Package\x00")) {
                    obj_index = @intCast(i);
                    break;
                }
            }

            std.debug.print("Patching SFSC export at index {d}\n", .{@"export".index});
            std.debug.print("  package name index: {d}\n", .{-obj_index});

            self.exports_table[@"export".index].ClassIndex = -obj_index;
            self.exports_table[@"export".index].SerialSize = 12;
        }

        if (sfsc_exports.items.len == 0) {
            std.log.info("No SFSC exports found, skipping patching", .{});
            return;
        }
        try self.writePatchedFile(filepath);
    }

    pub fn writePatchedFile(self: *Parser, original_filepath: []const u8) !void {
        const patched_filename = original_filepath; //try generatePatchedFilename(self.allocator, original_filepath);
        //defer self.allocator.free(patched_filename);

        std.log.info("Creating patched file: {s}", .{patched_filename});

        const cwd = fs.cwd();

        try cwd.copyFile(original_filepath, cwd, patched_filename, .{});

        const patched_file = try cwd.openFile(patched_filename, .{ .mode = .read_write });
        defer patched_file.close();

        var buffer: [4096]u8 = undefined;
        var fw = patched_file.writer(&buffer);
        const w = &fw.interface;

        try fw.seekTo(self.summary.export_offset);

        // Skip the first dummy export (index 0)
        for (self.exports_table[1..], 1..) |@"export", i| {
            if (i == 197 or i == 198) {
                std.debug.print("Currently at offset 0x{X}\n", .{fw.pos});
            }

            try @"export".write(w);
        }

        // try w.writeInt(u32, 0x69696969, .little);

        // Flush any remaining buffered data
        try fw.interface.flush();

        std.log.info("Successfully patched file!", .{});
    }

    fn generatePatchedFilename(allocator: mem.Allocator, original_filepath: []const u8) ![]u8 {
        // Find the last dot for the extension
        if (mem.lastIndexOf(u8, original_filepath, ".")) |dot_index| {
            const basename = original_filepath[0..dot_index];
            const extension = original_filepath[dot_index..];

            return try std.fmt.allocPrint(allocator, "{s}_patched{s}", .{ basename, extension });
        } else {
            // No extension found, just append _patched
            return try std.fmt.allocPrint(allocator, "{s}_patched", .{original_filepath});
        }
    }

    pub fn parse(self: *Parser) !void {
        var buffer: [4096]u8 = undefined;
        var fr = self.file.reader(&buffer);
        const r: *std.Io.Reader = &fr.interface;

        self.summary = try ue.FPackageFileSummary.read(r, self.allocator);
        self.summary.print();

        // Read the names table
        try fr.seekTo(self.summary.name_offset);
        self.names_table = try self.allocator.alloc(ue.FNameEntry, self.summary.name_count);

        for (self.names_table) |*name_entry| {
            name_entry.* = try ue.FNameEntry.read(r, self.allocator);
        }

        // Read the imports table
        try fr.seekTo(self.summary.import_offset);
        self.imports_table = try self.allocator.alloc(ue.FObjectImport, self.summary.import_count + 1);

        self.imports_table[0] = ue.FObjectImport{};
        for (self.imports_table[1..]) |*import| {
            import.* = try ue.FObjectImport.read(r);
        }

        // Read the exports table
        try fr.seekTo(self.summary.export_offset);
        self.exports_table = try self.allocator.alloc(ue.FObjectExport, self.summary.export_count + 1);

        self.exports_table[0] = ue.FObjectExport{};
        for (self.exports_table[1..]) |*@"export"| {
            @"export".* = try ue.FObjectExport.read(r);
        }

        // TODO: read the depends table

    }

    pub fn deinit(self: *Parser) void {
        // Free the generations and folder name
        self.allocator.free(self.summary.generations);
        self.summary.folder_name.deinit(self.allocator);

        // Free all the names from the names table
        for (self.names_table) |name_entry| {
            name_entry.name.deinit(self.allocator);
        }
        self.allocator.free(self.names_table);

        // Free the rest
        self.allocator.free(self.imports_table);
        self.allocator.free(self.exports_table);
        // self.allocator.free(self.depends_table);
    }
};

pub fn main() !void {
    var allocator = std.heap.DebugAllocator(.{}).init;
    const gpa = allocator.allocator();
    defer _ = allocator.deinit();

    const args = try std.process.argsAlloc(gpa);
    defer std.process.argsFree(gpa, args);

    if (args.len < 2) {
        std.log.err("Usage: {s} <file.upk>", .{args[0]});
        return;
    }

    const filepath = args[1];
    std.log.info("filepath: {s}", .{filepath});

    const cwd = fs.cwd();
    const file = cwd.openFile(filepath, .{}) catch |e| {
        return switch (e) {
            error.FileNotFound => std.log.err("File not found!", .{}),
            else => std.log.err("Error: {}", .{e}),
        };
    };
    errdefer file.close();

    var parser = try Parser.init(gpa, file);
    defer parser.deinit();

    try parser.parse();
    try parser.patchSFSC(filepath);
}
