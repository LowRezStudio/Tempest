const std = @import("std");

const Fields = @import("tokens.zig").Fields;
const Functions = @import("tokens.zig").Functions;

const parser_mode = enum {
    serialize,
    deserialize,
};

const parser_version = enum {
    legacy,
    modern,
};

const parser_options = struct {
    allocator: std.mem.Allocator,
    fields: Fields,
    functions: Functions,
    mode: parser_mode,
    version: parser_version,
    is_encrypted: bool,
};

pub const Parser = struct {
    options: parser_options,
    reader: *std.Io.Reader,
    writer: *std.Io.Writer,

    pub fn init(options: parser_options) !Parser {
        // reader
        // writer

        return .{
            .options = options,
            .reader = undefined,
            .writer = undefined,
        };
    }

    pub fn deserialize(self: *const Parser) !void {
        _ = self;
        std.debug.print("TODO: implement deserialize\n", .{});
    }

    pub fn serialize(self: *const Parser) !void {
        _ = self;
        std.debug.print("TODO: implement serialize\n", .{});
    }

    pub fn printDebug(self: *const Parser) !void {
        const options = self.options;

        for (options.fields.list) |field| {
            std.debug.print("Field: {d} -> {s} ({s} -> {s})\n", .{ field.index, field.name, @tagName(field.type), @tagName(field.netid_type) });
        }
        for (options.functions.list) |function| {
            std.debug.print("Function: {d} -> {s} ({x})\n", .{ function.index, function.name, function.hash });
        }
        std.debug.print("Fields: {d}\n", .{options.fields.list.len});
        std.debug.print("Functions: {d}\n", .{options.functions.list.len});

        const test1 = try options.functions.findByHash(0x76513d71);
        const test2 = try options.functions.findByIndex(818);
        std.debug.print("Test1: {s}\n", .{test1.name});
        std.debug.print("Test2: {s}\n", .{test2.name});
    }
};
