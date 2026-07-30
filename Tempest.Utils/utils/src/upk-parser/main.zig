const std = @import("std");
const Io = std.Io;
const builtin = @import("builtin");

const Parser = @import("parser.zig");
const TArray = @import("unreal.zig").TArray;

const EPackageFlags = @import("package.zig").EPackageFlags;

pub fn main(init: std.process.Init) !void {
    var allocator = init.arena.allocator();
    if (builtin.mode == .Debug) {
        allocator = init.gpa;
    }

    const io = init.io;
    const args = try init.minimal.args.toSlice(init.arena.allocator());

    if (args.len < 2) {
        std.debug.print("Usage: {s} <file.upk>\n", .{args[0]});
        return;
    }

    var p = try Parser.init(io, args[1], allocator);
    defer p.deinit();

    try p.parse();

    std.debug.print("{f}\n", .{p.package_file_summary});
}
