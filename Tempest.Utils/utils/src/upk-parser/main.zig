const builtin = @import("builtin");
const std = @import("std");
const Io = std.Io;

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
    for (p.exports_table) |exports| {
        std.debug.print("{f}\n", .{exports});
    }
}
