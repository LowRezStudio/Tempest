const std = @import("std");
const Lua = @import("luajit").Lua;

export fn op_console_print(bytes: [*:0]const u8) void {
    var out = std.fs.File.stdout().writerStreaming(&.{});
    const len = std.mem.len(bytes);
    out.interface.writeAll(bytes[0..len]) catch {};
}

pub fn init(lua: *Lua) !void {
    try lua.doString(@embedFile("lib.lua"));
}
