const std = @import("std");
const fs = std.fs;

const Fields = @import("tokens.zig").Fields;
const Functions = @import("tokens.zig").Functions;
const marshal = @import("marshal.zig");
const Tokens = @import("tokens.zig").Tokens;

const parser_mode = enum {
    serialize,
    deserialize,
};

const parser_version = enum {
    legacy,
    modern,
};

const parser_options = struct {
    file_path: []const u8,
    mode: parser_mode,
    version: parser_version,
    obscure: bool,
};

pub const Parser = struct {
    allocator: std.mem.Allocator,
    options: parser_options,
    fields: Fields,
    functions: Functions,
    reader: *std.Io.Reader,
    writer: *std.Io.Writer,

    pub fn init(allocator: std.mem.Allocator, options: parser_options) !Parser {
        // reader
        // writer

        return .{
            .allocator = allocator,
            .options = options,
            .fields = Tokens.global.fields,
            .functions = Tokens.global.functions,
            .reader = undefined,
            .writer = undefined,
        };
    }

    pub fn deserialize(self: *const Parser) !void {
        var package = try marshal.CPackage.init(self.allocator);
        defer package.deinit(self.allocator);

        _ = try package.readFromFile(self.allocator, self.options.file_path, self.options.obscure);

        _ = try package.setPlace(self.allocator, 0);
        var marsh = marshal.CMarshal.init(0);

        _ = try marsh.load(&package);

        std.debug.print("{f}\n", .{package});
        std.debug.print("{f}\n", .{marsh});

        std.debug.print("TODO: implement deserialize\n", .{});
    }

    pub fn serialize(self: *const Parser) !void {
        _ = self;
        std.debug.print("TODO: implement serialize\n", .{});
    }

    pub fn printDebug(self: *const Parser) !void {
        for (self.fields.list) |field| {
            std.debug.print("Field: {d} -> {s} ({s} -> {s})\n", .{ field.index, field.name, @tagName(field.type), @tagName(field.netid_type) });
        }
        for (self.functions.list) |function| {
            std.debug.print("Function: {d} -> {s} ({x})\n", .{ function.index, function.name, function.hash });
        }
        std.debug.print("Fields: {d}\n", .{self.fields.list.len});
        std.debug.print("Functions: {d}\n", .{self.functions.list.len});
    }
};
