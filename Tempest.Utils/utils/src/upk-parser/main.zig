const std = @import("std");
const Io = std.Io;
const builtin = @import("builtin");

const Parser = @import("parser.zig");

pub fn main(init: std.process.Init) !void {
    var allocator = init.arena.allocator();
    if (builtin.mode == .Debug) {
        allocator = init.gpa;
    }

    const io = init.io;
    const args = try init.minimal.args.toSlice(init.arena.allocator());

    var p = try Parser.init(io, allocator, args[1], .{ .verbose = true });
    defer p.deinit();

    try p.parse();

    for (p.exports_table[1..]) |exports| {
        std.debug.print("{f}\n", .{exports});
    }

    for (p.imports_table[1..]) |imports| {
        std.debug.print("{f}\n", .{imports});
    }

    // for (p.names_table) |name_entry| {
    //     std.debug.print("{f}\n", .{name_entry.name});
    // }
}
