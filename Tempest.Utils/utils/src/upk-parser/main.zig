const std = @import("std");
const Io = std.Io;
const builtin = @import("builtin");

const Parser = @import("parser.zig");
const Tree = @import("tree.zig");

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

    var map = try Tree.buildChildrenMap(&p, allocator);
    defer {
        var it = map.valueIterator();
        while (it.next()) |list| list.deinit(allocator);
        map.deinit();
    }

    var stdout_buf: [4096]u8 = undefined;
    var stdout_writer = std.Io.File.stdout().writer(io, &stdout_buf);
    try Tree.printTree(&p, &map, &stdout_writer.interface, allocator);
    try stdout_writer.interface.flush();

    // for (p.names_table) |name_entry| {
    //     std.debug.print("{f}\n", .{name_entry.name});
    // }

    // Test save
    try p.save(io, "test.upk");
}
