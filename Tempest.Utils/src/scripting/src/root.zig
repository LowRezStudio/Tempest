const std = @import("std");
const Lua = @import("luajit").Lua;
const windows = std.os.windows;

pub const unreal = @import("unreal/root.zig");

extern "kernel32" fn AllocConsole() callconv(.winapi) windows.BOOL;

fn initScript(allocator: std.mem.Allocator) !void {
    const lua = try Lua.init(allocator);
    defer lua.deinit();

    lua.openLibs();

    try @import("ext/console/root.zig").init(lua);
    try @import("ext/memory/root.zig").init(lua);

    lua.doString(
        \\print("[Lua] Hello World!")
    ) catch |err| switch (err) {
        error.Runtime => {
            std.debug.print("[Zig] Runtime error: {s}\n", .{lua.toString(-1) catch "unknown"});
            return;
        },
        else => {
            std.debug.print("[Zig] Unknown error: {s}\n", .{@errorName(err)});
            return;
        },
    };
}

fn main() !void {
    _ = AllocConsole();

    const allocator = std.heap.page_allocator;

    try initScript(allocator);
}

pub export fn DllMain(
    hinstDLL: windows.HINSTANCE,
    fdwReason: u32,
    lpvReserved: ?*anyopaque,
) windows.BOOL {
    _ = hinstDLL;
    _ = lpvReserved;

    if (fdwReason == 1) {
        _ = std.Thread.spawn(.{}, main, .{}) catch {};
    }

    return windows.TRUE;
}
