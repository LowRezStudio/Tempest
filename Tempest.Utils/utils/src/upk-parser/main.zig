const std = @import("std");
const Io = std.Io;
const builtin = @import("builtin");

const debug = @import("debug.zig");
const Parser = @import("parser.zig");

pub fn main(init: std.process.Init) !void {
    var allocator = init.arena.allocator();
    if (builtin.mode == .Debug) {
        allocator = init.gpa;
    }

    const io = init.io;
    const args = try init.minimal.args.toSlice(init.arena.allocator());

    if (args.len < 2) {
        std.debug.print("Usage: {s} <file.upk> [-report] [-dump <out.upk>]\n", .{args[0]});
        return;
    }

    var p = try Parser.init(io, args[1], allocator);
    defer p.deinit();

    try p.parse();

    if (args.len >= 4 and std.mem.eql(u8, args[2], "-dump")) {
        try std.Io.Dir.cwd().writeFile(init.io, .{
            .sub_path = args[3],
            .data = p.data_buffer,
            .flags = .{ .truncate = true },
        });
        std.debug.print("dumped {d} bytes to {s}\n", .{ p.data_buffer.len, args[3] });
    }

    std.debug.print("{f}\n", .{p.package_file_summary});

    std.debug.print("parsed: {d} names, {d} imports, {d} exports\n", .{
        p.name_map.len,
        p.import_map.len,
        p.export_map.len,
    });

    if (args.len >= 3 and std.mem.eql(u8, args[2], "-report")) {
        debug.generatePackageReport(&p);
    }
}
