const std = @import("std");
const log = std.log;
const windows = std.os.windows;
const builtin = @import("builtin");
const arch = builtin.cpu.arch;

const detourz = @import("detourz");
const memory = @import("memory");

pub const THISCALL: std.builtin.CallingConvention =
    if (arch == .x86)
        .{ .x86_thiscall = .{} }
    else
        .c;

extern "kernel32" fn AllocConsole() callconv(.winapi) windows.BOOL;
extern "kernel32" fn FreeConsole() callconv(.winapi) windows.BOOL;
extern "kernel32" fn FreeLibraryAndExitThread(hLibModule: windows.HMODULE, dwExitCode: u32) callconv(.winapi) noreturn;
extern "kernel32" fn GetCurrentThread() callconv(.winapi) windows.HANDLE;
extern "kernel32" fn Sleep(dwMilliseconds: u32) callconv(.winapi) void;

// void __thiscall sub_64930(_DWORD *this, _DWORD *a2)
const AddNetPackage = *const fn (pThis: ?*anyopaque, a2: u32) callconv(THISCALL) void;

// int __thiscall sub_C732D0(_DWORD *this, char a2)
const LoadFile = *const fn (pThis: ?*anyopaque, fullLoad: u8) callconv(THISCALL) c_int;

var addNetPackage: AddNetPackage = undefined;
var gAssemblyManager: ?*anyopaque = undefined;
var loadFile: LoadFile = undefined;
var isLoaded: bool = false;

fn hookAddNetPackage(pThis: ?*anyopaque, a2: u32) callconv(THISCALL) void {
    if (!isLoaded) {
        const result = loadFile(gAssemblyManager, 1);
        if (result != 0) {
            isLoaded = true;
            std.debug.print("[AsmLoader] Loaded assembly!\n", .{});
        } else {
            std.debug.print("[AsmLoader] Failed to load assembly!\n", .{});
        }
    }

    _ = addNetPackage(pThis, a2);
}

fn main(hinstDLL: windows.HINSTANCE) !void {
    _ = AllocConsole();

    const module = try memory.Module.init(null);
    const base_addr = module.getHandle().get();

    const scanner = module.scanner();
    if (scanner.string("Assembly load started")) |ref| {
        const gAssemblyManagerRef = if (builtin.cpu.arch == .x86) blk: {
            const call = try ref.scanFor(&.{memory.Mnemonic.MOV_ECX}, true, 0);
            break :blk call;
        } else blk: {
            break :blk try ref.scanFor(&.{memory.Mnemonic.LEA}, true, 1);
        };

        gAssemblyManager = if (builtin.cpu.arch == .x86) blk: {
            const address = try gAssemblyManagerRef.address.?.absoluteOffset(1);
            break :blk @ptrFromInt(address.get());
        } else blk: {
            const address = try gAssemblyManagerRef.address.?.relativeOffset(2);
            break :blk @ptrFromInt(address.get());
        };

        const loadFileRef = try ref.scanFor(&.{memory.Mnemonic.CALL}, true, 1);
        loadFile = blk: {
            const address = try loadFileRef.address.?.relativeOffset(1);
            break :blk @ptrFromInt(address.get());
        };
    } else |err| {
        std.debug.print("Failed to find string! err:{}\n", .{err});
    }

    if (scanner.string("Skipping adding %s to NetPackages")) |ref| {
        const addNetPackageRef = try ref.scanFor(&.{ memory.Mnemonic.INT3, memory.Mnemonic.INT3 }, false, 0);
        addNetPackage = @ptrFromInt(addNetPackageRef.address.?.get() + 2);
    } else |err| {
        std.debug.print("Failed to find string! err:{}\n", .{err});
    }

    std.debug.print("Base address: 0x{x}\n", .{base_addr});
    std.debug.print("Found addNetworkPackage: 0x{x}\n", .{@intFromPtr(addNetPackage)});
    std.debug.print("Found gAssemblyManager: 0x{x}\n", .{@intFromPtr(gAssemblyManager)});
    std.debug.print("Found loadFile: 0x{x}\n", .{@intFromPtr(loadFile)});

    try detourz.transactionBegin();
    try detourz.updateThread(GetCurrentThread());

    try detourz.attach(@ptrCast(&addNetPackage), @constCast(&hookAddNetPackage));

    try detourz.transactionCommit();

    // _ = FreeConsole();
    // FreeLibraryAndExitThread(@ptrCast(hinstDLL), 0);
    _ = hinstDLL;
}

pub export fn DllMain(
    hinstDLL: windows.HINSTANCE,
    fdwReason: u32,
    lpvReserved: ?*anyopaque,
) windows.BOOL {
    _ = lpvReserved;

    if (fdwReason == 1) {
        // _ = std.Thread.spawn(.{}, main, .{hinstDLL}) catch return windows.FALSE;
        _ = main(hinstDLL) catch return windows.FALSE;
    }

    return windows.TRUE;
}
