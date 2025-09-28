const std = @import("std");
const luajit = @import("luajit");
const windows = std.os.windows;
const Lua = luajit.Lua;

extern "kernel32" fn AllocConsole() callconv(.winapi) windows.BOOL;

fn my_print(lua: *Lua) callconv(.c) i32 {
    const n = lua.getTop(); // Number of arguments

    // Get the global tostring function
    _ = lua.getGlobal("tostring");

    var i: i32 = 1;
    while (i <= n) : (i += 1) {
        // Push tostring function
        lua.pushValue(-1);
        // Push argument
        lua.pushValue(i);
        // Call tostring(arg)
        lua.call(1, 1);

        // Get result as string
        const s = lua.toString(-1) catch {
            _ = lua.pushString("'tostring' must return a string to 'print'");
            lua.raiseError();
            return 0;
        };

        if (i > 1) {
            std.debug.print("\t", .{});
        }
        std.debug.print("{s}", .{s});
        lua.pop(1); // Remove result
    }
    std.debug.print("\n", .{});

    return 0; // Number of results
}

fn main() !void {
    _ = AllocConsole();

    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    const lua = try Lua.init(gpa.allocator());
    defer lua.deinit();

    lua.openBaseLib();

    lua.pushCFunction(my_print);
    lua.setGlobal("print");

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
