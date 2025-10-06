const std = @import("std");
const log = std.log;
const windows = std.os.windows;
const builtin = @import("builtin");
const arch = builtin.cpu.arch;

const detourz = @import("detourz");

pub const THISCALL: std.builtin.CallingConvention =
    if (arch == .x86)
        .{ .x86_fastcall = .{} }
    else
        .c;

extern "kernel32" fn AllocConsole() callconv(.winapi) windows.BOOL;
extern "kernel32" fn FreeConsole() callconv(.winapi) windows.BOOL;
extern "kernel32" fn FreeLibraryAndExitThread(hLibModule: windows.HMODULE, dwExitCode: u32) callconv(.winapi) noreturn;
extern "kernel32" fn GetModuleHandleA(lpModuleName: ?[*:0]const u8) callconv(.winapi) ?windows.HMODULE;
extern "kernel32" fn GetCurrentThread() callconv(.winapi) windows.HANDLE;
extern "kernel32" fn Sleep(dwMilliseconds: u32) callconv(.winapi) void;

const AddNetObject = *const fn (self: ?*anyopaque, edx: ?*anyopaque, a2: ?*anyopaque) callconv(THISCALL) ?*anyopaque;
// const AddNetObject = if (arch == .x86)
//     *const fn (self: ?*anyopaque, edx: ?*anyopaque, a2: ?*anyopaque) callconv(THISCALL) void
// else
//     *const fn (self: ?*anyopaque, a2: ?*anyopaque) callconv(THISCALL) void;

const AssemblyLoad = *const fn (self: ?*anyopaque, edx: ?*anyopaque, bFullLoad: windows.CHAR) callconv(THISCALL) ?*anyopaque;
// const AssemblyLoad = if (arch == .x86)
//     *const fn (self: ?*anyopaque, edx: ?*anyopaque, bFullLoad: windows.CHAR) callconv(THISCALL) c_int
// else
//     *const fn (self: ?*anyopaque, bFullLoad: windows.CHAR) callconv(THISCALL) c_int;

var oAddNetObject: AddNetObject = undefined;
var oAssemblyLoad: AssemblyLoad = undefined;
var AssemblyManager: ?*anyopaque = undefined;
var bLoadedAssembly: bool = false;

fn hAddNetObject(self: ?*anyopaque, edx: ?*anyopaque, a2: ?*anyopaque) callconv(THISCALL) void {
    std.debug.print("AddNetObject: called\n", .{});

    const fmt =
        \\ this: 0x{x}
        \\ edx: 0x{x}
        \\ a2: 0x{x}
        \\
    ;

    std.debug.print(fmt, .{
        @intFromPtr(self),
        @intFromPtr(edx),
        @intFromPtr(a2),
    });

    // if (!bLoadedAssembly) {
    //     std.debug.print("Calling AssemblyLoad\n", .{});
    //         if (oAssemblyLoad(AssemblyManager, 1) != 0) {
    //             bLoadedAssembly = true;
    //         }
    //     }
    // }

    Sleep(5000);

    _ = oAddNetObject(self, edx, a2);
}

fn main(hinstDLL: windows.HINSTANCE) !void {
    _ = AllocConsole();

    std.debug.print("[Zig] Hello from ASM Loader!\n", .{});

    const module_base = GetModuleHandleA(null);
    const base_addr = @intFromPtr(module_base);

    oAddNetObject = @ptrFromInt(base_addr + 0x64930);
    oAssemblyLoad = @ptrFromInt(base_addr + 0xC732D0);
    AssemblyManager = @ptrFromInt(base_addr + 0x21B8F98);

    const fmt =
        \\ Base address: 0x{x}
        \\ AddNetObject: 0x{x} (0x{x})
        \\ AssemblyLoad: 0x{x} (0x{x})
        \\ AssemblyManager: 0x{x} (0x{x})
        \\
    ;

    std.debug.print(fmt, .{
        base_addr,
        @intFromPtr(oAddNetObject),
        @intFromPtr(oAddNetObject) - base_addr,

        @intFromPtr(oAssemblyLoad),
        @intFromPtr(oAssemblyLoad) - base_addr,

        @intFromPtr(AssemblyManager),
        @intFromPtr(AssemblyManager) - base_addr,
    });

    // Detour transaction
    try detourz.transactionBegin();
    try detourz.updateThread(GetCurrentThread());

    try detourz.attach(@ptrCast(&oAddNetObject), @constCast(&hAddNetObject));

    try detourz.transactionCommit();

    _ = FreeConsole();
    FreeLibraryAndExitThread(@ptrCast(hinstDLL), 0);

    // _ = hinstDLL;
}

pub export fn DllMain(
    hinstDLL: windows.HINSTANCE,
    fdwReason: u32,
    lpvReserved: ?*anyopaque,
) windows.BOOL {
    _ = lpvReserved;

    if (fdwReason == 1) {
        _ = std.Thread.spawn(.{}, main, .{hinstDLL}) catch return windows.FALSE;
    }

    return windows.TRUE;
}
