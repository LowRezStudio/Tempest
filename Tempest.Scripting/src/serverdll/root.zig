const std = @import("std");
const windows = std.os.windows;

extern "kernel32" fn AllocConsole() callconv(.winapi) windows.BOOL;
extern "kernel32" fn FreeLibrary(hLibModule: windows.HMODULE) callconv(.winapi) windows.BOOL;

fn main(hinstDLL: windows.HINSTANCE) !void {
    _ = AllocConsole();

    std.debug.print("Hello World!\n", .{});

    _ = FreeLibrary(hinstDLL);
}

pub export fn DllMain(
    hinstDLL: windows.HINSTANCE,
    fdwReason: u32,
    lpvReserved: ?*anyopaque,
) windows.BOOL {
    _ = lpvReserved;

    if (fdwReason == 1) {
        _ = std.Thread.spawn(.{}, main, .{hinstDLL}) catch {};
    }

    return windows.TRUE;
}
