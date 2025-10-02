const std = @import("std");
const Lua = @import("luajit").Lua;
const windows = std.os.windows;

extern "kernel32" fn AllocConsole() callconv(.winapi) windows.BOOL;

fn main() !void {
    _ = AllocConsole();

    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    const lua = try Lua.init(gpa.allocator());
    defer lua.deinit();

    lua.openLibs();

    try @import("ext/console/root.zig").init(lua);

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
