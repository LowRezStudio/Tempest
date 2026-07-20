const std = @import("std");
const Io = std.Io;

const Tempest_Utils = @import("Tempest_Utils");
const minilzo = Tempest_Utils.minilzo;

pub fn main(init: std.process.Init) !void {
    if (minilzo.init() == minilzo.LZOError.InitFailed) {
        std.log.err("minilzo init failed", .{});
        return;
    }
    std.log.info("minilzo initialized", .{});

    const arena: std.mem.Allocator = init.arena.allocator();

    const args = try init.minimal.args.toSlice(arena);
    for (args) |arg| {
        std.log.info("arg: {s}", .{arg});
    }

    const io = init.io;

    var stdout_buffer: [1024]u8 = undefined;
    var stdout_file_writer: Io.File.Writer = .init(.stdout(), io, &stdout_buffer);
    const stdout_writer = &stdout_file_writer.interface;

    try stdout_writer.writeAll("Hello, world!\n");

    try stdout_writer.flush(); // Don't forget to flush!
}
