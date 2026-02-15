const std = @import("std");

const FieldEntry = @import("tokens.zig").FieldEntry;
const FunctionDetail = @import("tokens.zig").FunctionDetail;

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
    fields: []FieldEntry,
    functions: []FunctionDetail,
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

    pub fn printDebug(self: *const Parser) void {
        const options = self.options;

        for (options.fields) |field| {
            std.debug.print("Field: {d} -> {s} ({s} -> {s})\n", .{ field.index, field.name, @tagName(field.type), @tagName(field.netid_type) });
        }
        for (options.functions) |function| {
            std.debug.print("Function: {d} -> {s} ({x})\n", .{ function.index, function.name, function.hash });
        }
        std.debug.print("Fields: {d}\n", .{options.fields.len});
        std.debug.print("Functions: {d}\n", .{options.functions.len});
    }
};
