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

    pub fn parse(self: *Parser) !void {
        var buffer: [32 * 1024]u8 = undefined;
        var fr = self.file.reader(&buffer);
        const r: *std.Io.Reader = &fr.interface;

        self.summary = try ue.FPackageFileSummary.read(r, self.allocator);
        self.summary.print();

        // Read the names table
        r.seek = self.summary.name_offset;
        self.names_table = try self.allocator.alloc(ue.FNameEntry, self.summary.name_count);

        for (self.names_table) |*name_entry| {
            name_entry.* = try ue.FNameEntry.read(r);
        }

        std.debug.print("Names Table:\n", .{});
        for (self.names_table) |name_entry| {
            std.debug.print("  {s}\n", .{name_entry.name.toString()});
        }

        std.debug.print("Names Table:\n", .{});
        for (self.names_table) |name_entry| {
            std.debug.print("  {s}\n", .{name_entry.name.toString()});
        }

        // TODO: read the imports table

        // Read the exports table
        r.seek = self.summary.export_offset;
        self.exports_table = try self.allocator.alloc(ue.FObjectExport, self.summary.export_count);

        for (self.exports_table) |*@"export"| {
            @"export".* = try ue.FObjectExport.read(r);
        }

        std.debug.print("Exports Table:\n", .{});
        for (self.exports_table) |@"export"| {
            std.debug.print("  {s}\n", .{self.names_table[@"export".NameTableIndex].name.toString()});
        }

        // TODO: read the depends table
    }

    pub fn deinit(self: *Parser) void {
        self.allocator.free(self.summary.generations);
        self.allocator.free(self.names_table);
        // self.allocator.free(self.imports_table);
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
}
