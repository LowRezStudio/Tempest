const std = @import("std");
const Io = std.Io;
const mem = std.mem;

const Tempest_Utils = @import("Tempest_Utils");
const minilzo = Tempest_Utils.minilzo;

const archive = @import("archive.zig");
const constants = @import("constants.zig");
const core = @import("core.zig");
const objects = @import("objects.zig");

pub const Parser = struct {
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

        std.debug.print("file_buffer: {d}\n", .{file_buffer.len});

        return Parser{
            .allocator = allocator,
            .file_buffer = file_buffer,
            .options = options,
        };
    }

    pub fn deinit(self: *Parser) void {
        self.allocator.free(self.file_buffer);
        self.allocator.free(self.names_table);
        self.allocator.free(self.imports_table);
        self.allocator.free(self.exports_table);
        for (self.depends_table) |d| self.allocator.free(d);
        self.allocator.free(self.depends_table);
        self.allocator.free(self.data_buffer);
        self.* = undefined;
    }
};
