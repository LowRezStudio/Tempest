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

    pub fn patchSFSC(self: *Parser) !void {
        var sfsc_exports = std.ArrayList(ue.FObjectExport).empty;
        defer sfsc_exports.deinit(self.allocator);

        for (self.exports_table) |@"export"| {
            if (std.mem.eql(u8, self.names_table[@"export".NameTableIndex].name.toString(), "SeekFreeShaderCache\x00")) {
                try sfsc_exports.append(self.allocator, @"export");
            }
        }

        std.debug.print("SFSC Exports:\n", .{});
        for (sfsc_exports.items) |@"export"| {
            const obj_type_name = if (@"export".ClassIndex < 0)
                self.names_table[self.imports_table[@abs(@"export".ClassIndex)].NameTableIndex].name.toString()
            else
                self.names_table[@abs(@"export".ClassIndex)].name.toString();

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
                self.names_table[@abs(@"export".NameTableIndex)].name.toString(),
                @"export".NameCount,
                @"export".NameCount,
                @"export".SerialSize,
                obj_type_name,
                @"export".SuperIndex,
                self.names_table[self.exports_table[@abs(@"export".SuperIndex)].NameTableIndex].name.toString(),
                self.exports_table[@abs(@"export".SuperIndex)].NameCount,
                @"export".ArchetypeIndex,
                self.names_table[self.exports_table[@abs(@"export".ArchetypeIndex)].NameTableIndex].name.toString(),
                self.exports_table[@abs(@"export".ArchetypeIndex)].NameCount,
            });
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
    try parser.patchSFSC();
}
