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
const AddNetworkPackage = *const fn (pThis: ?*anyopaque, a2: u32) callconv(THISCALL) void;

// int __thiscall sub_C732D0(_DWORD *this, char a2)
const LoadFile = *const fn (pThis: ?*anyopaque, fullLoad: u8) callconv(THISCALL) c_int;

var addNetworkPackage: AddNetworkPackage = undefined;
var gAssemblyManager: ?*anyopaque = undefined;
var loadFile: LoadFile = undefined;
var isLoaded: bool = false;

fn hookAddNetworkPackage(pThis: ?*anyopaque, a2: u32) callconv(THISCALL) void {
    if (!isLoaded) {
        const result = loadFile(gAssemblyManager, 1);
        if (result != 0) {
            isLoaded = true;
            std.debug.print("[AsmLoader] Loaded assembly!\n", .{});
        } else {
            std.debug.print("[AsmLoader] Failed to load assembly!\n", .{});
        }
    }

    _ = addNetworkPackage(pThis, a2);
}

fn main(hinstDLL: windows.HINSTANCE) !void {
    _ = AllocConsole();

    std.debug.print("[AsmLoader] Hello from Zig!\n", .{});

    const module = try memory.Module.init(null);
    const base_addr = module.getHandle().get();

    // TESTS //////////////////////////////////////////////////////////////////
    if (module.scanner().pattern(std.heap.page_allocator, "50 8D ? ? ? ? A3 00 00 00 00 8B F9 33")) |foo| {
        defer std.heap.page_allocator.free(foo);

        std.debug.print("[AsmLoader] Found {d} pattern(s)!\n", .{foo.len});
        for (foo, 0..) |address, i| {
            std.debug.print("Scan result[{d}]: 0x{x} (0x{x})\n", .{ i, address.get(), address.get() - base_addr });
        }
        std.debug.print("\n", .{});
    } else |err| {
        std.debug.print("[AsmLoader] Failed to find pattern! err:{}\n", .{err});
    }

    if (module.scanner().string(std.heap.page_allocator, "ASSEMBLY", false)) |ref| {
        defer std.heap.page_allocator.free(ref);
        std.debug.print("Found string ref: {any}\n", .{ref});
    } else |err| {
        std.debug.print("Failed to find string! err:{}\n", .{err});
    }

    const section = try module.section(std.heap.page_allocator, ".text");
    std.debug.print("Section: {any}\n", .{section});
    std.debug.print("Section: {s}\n", .{section.name});

    const sections = try section.getAllSections(std.heap.page_allocator);
    defer std.heap.page_allocator.free(sections);

    for (sections, 0..) |s, i| {
        std.debug.print("Section [{d}]: {s}\n", .{ i, s.getName() });
    }

    const text_section = try section.getSection(std.heap.page_allocator, ".text");

    std.debug.print("Virtual Address: 0x{x}\n", .{text_section.VirtualAddress});
    std.debug.print("Size: 0x{x}\n", .{text_section.VirtualSize});
    std.debug.print("Name: {s}\n", .{text_section.getName()});

    ///////////////////////////////////////////////////////////////////////////

    // string ref "Assembly load started"
    addNetworkPackage = @ptrFromInt(base_addr + 0x64930);
    gAssemblyManager = @ptrFromInt(base_addr + 0x21B8F98);
    loadFile = @ptrFromInt(base_addr + 0xC732D0);

    const fmt =
        \\ Base address: 0x{x}
        \\ addNetworkPackage: 0x{x} (0x{x})
        \\ gAssemblyManager: 0x{x} (0x{x})
        \\ loadFile: 0x{x} (0x{x})
        \\
    ;

    std.debug.print(fmt, .{
        base_addr,
        @intFromPtr(addNetworkPackage),
        @intFromPtr(addNetworkPackage) - base_addr,

        @intFromPtr(gAssemblyManager),
        @intFromPtr(gAssemblyManager) - base_addr,

        @intFromPtr(loadFile),
        @intFromPtr(loadFile) - base_addr,
    });

    try detourz.transactionBegin();
    try detourz.updateThread(GetCurrentThread());

    try detourz.attach(@ptrCast(&addNetworkPackage), @constCast(&hookAddNetworkPackage));

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
        _ = std.Thread.spawn(.{}, main, .{hinstDLL}) catch return windows.FALSE;
        // _ = main(hinstDLL) catch return windows.FALSE;
    }

    return windows.TRUE;
}
