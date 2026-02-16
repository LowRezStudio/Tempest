const std = @import("std");
const fs = std.fs;

const Fields = @import("tokens.zig").Fields;
const Functions = @import("tokens.zig").Functions;
const marshal = @import("marshal.zig");

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

    pub fn init(allocator: std.mem.Allocator, fields: Fields, functions: Functions, options: parser_options) !Parser {
        // reader
        // writer

        return .{
            .allocator = allocator,
            .options = options,
            .fields = fields,
            .functions = functions,
            .reader = undefined,
            .writer = undefined,
        };
    }

    pub fn deserialize(self: *const Parser) !void {
        var package = try marshal.CPackage.init(self.allocator);
        _ = try package.readFromFile(self.allocator, self.options.file_path, self.options.obscure);

        // TODO: remove this test stuff
        std.debug.print("{f}\n", .{package});
        var current_packet: ?*marshal.CPackPacket = package.packets;
        var i: usize = 0;
        while (current_packet) |packet| {
            if (packet.next == null) break;

            i += 1;
            current_packet = packet.next;
        }
        std.debug.print("Packets in this package: {d}\n", .{i});

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
