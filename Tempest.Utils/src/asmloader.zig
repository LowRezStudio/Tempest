const std = @import("std");
const log = std.log;
const windows = std.os.windows;
const arch = @import("builtin").cpu.arch;

const detourz = @import("detourz");
const memory = @import("memory");
const zydis = @import("zydis");

extern "kernel32" fn AllocConsole() callconv(.winapi) windows.BOOL;
extern "kernel32" fn FreeConsole() callconv(.winapi) windows.BOOL;
extern "kernel32" fn FreeLibraryAndExitThread(hLibModule: windows.HMODULE, dwExitCode: u32) callconv(.winapi) noreturn;
extern "kernel32" fn GetCurrentThread() callconv(.winapi) windows.HANDLE;

pub const THISCALL: std.builtin.CallingConvention =
    if (arch == .x86)
        .{ .x86_thiscall = .{} }
    else
        .c;

// void __thiscall UPackage::AddNetPackage(_DWORD *this, _DWORD *a2)
const AddNetPackage = *const fn (this: ?*anyopaque, a2: usize) callconv(THISCALL) void;

// int __thiscall CTgAssemblyManager::LoadFile(_DWORD *this, char a2)
const LoadFile = *const fn (this: ?*anyopaque, fullLoad: u8) callconv(THISCALL) c_int;

var original_addnetpackage: AddNetPackage = undefined;
var assembly_manager: ?*anyopaque = undefined;
var loadfile: LoadFile = undefined;

var is_loaded = std.atomic.Value(bool).init(false);

fn hookAddNetPackage(this: ?*anyopaque, a2: usize) callconv(THISCALL) void {
    if (!is_loaded.load(.acquire)) {
        log.info("[AsmLoader] Loading assembly...", .{});
        const result = @call(.auto, loadfile, .{ assembly_manager, 1 });
        if (result != 0) {
            log.info("[AsmLoader] Loaded assembly!", .{});
            is_loaded.store(true, .release);
        } else {
            log.info("[AsmLoader] Failed to load assembly!", .{});
        }
    }

    _ = original_addnetpackage(this, a2);
}

fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    const allocator = arena.allocator();
    defer arena.deinit();

    _ = AllocConsole();

    log.info("[AsmLoader] Starting...", .{});

    const module = try memory.Module.init(null);
    const scanner = module.scanner();

    log.info("[AsmLoader] Scanning for assembly manager...", .{});
    assembly_manager = blk: {
        const strings = try scanner.string(allocator, "Assembly load started\x00");
        const string = strings[0];

        const call = try scanner.findNext(string.address.?.get(), .{
            .inst = zydis.ZYDIS_MNEMONIC_CALL,
        });

        const address = try scanner.findNext(call.get(), .{
            .instructions = &.{ zydis.ZYDIS_MNEMONIC_LEA, zydis.ZYDIS_MNEMONIC_MOV },
            .dst_reg = if (arch == .x86_64) zydis.ZYDIS_REGISTER_RCX else zydis.ZYDIS_REGISTER_ECX, // NOTE: kms
            .limit = 4,
        });

        log.info("[AsmLoader] Scanning for loadFile...", .{});
        const loadfile_ref = try scanner.findNext(address.get(), .{
            .inst = zydis.ZYDIS_MNEMONIC_CALL,
        });

        loadfile = loadfile: {
            const result = try loadfile_ref.getAddress();
            break :loadfile result.AsPtr(LoadFile);
        };

        const value: usize = if (address.getAddress()) |addr|
            addr.get()
        else |_|
            try address.getData(1);

        log.info("[AsmLoader] Found assembly_manager: {x}", .{value});

        break :blk @ptrFromInt(value);
    };

    log.info("[AsmLoader] Scanning for addNetworkPackage...", .{});
    original_addnetpackage = blk: {
        const anchor = anchor: {
            if (arch == .x86_64) {
                const addresses = try scanner.pattern(allocator, "74 ? 48 89 5C 24 ? 48 8B DF");
                break :anchor addresses[0];
            } else {
                const addresses = try scanner.pattern(allocator, "74 ? 56 8B F7 8B 46");
                break :anchor addresses[0];
            }
        };

        const call = try scanner.findNext(anchor.get(), .{
            .inst = zydis.ZYDIS_MNEMONIC_CALL,
            .skip = 1,
        });

        const target = try call.getAddress();
        break :blk target.AsPtr(AddNetPackage);
    };

    log.info(
        \\Base address             : 0x{x}
        \\Found addNetPackage      : 0x{x}
        \\Found assembly_manager   : 0x{x}
        \\Found loadFile           : 0x{x}
        \\
    , .{
        module.base_address,
        @intFromPtr(original_addnetpackage) - module.base_address,
        @intFromPtr(assembly_manager) - module.base_address,
        @intFromPtr(loadfile) - module.base_address,
    });

    try detourz.transactionBegin();
    try detourz.updateThread(GetCurrentThread());

    try detourz.attach(@ptrCast(&original_addnetpackage), @constCast(&hookAddNetPackage));

    try detourz.transactionCommit();
}

pub export fn DllMain(
    inst: windows.HINSTANCE,
    reason: u32,
    reserved: ?*anyopaque,
) windows.BOOL {
    _ = inst;
    _ = reserved;

    if (reason == 1) {
        _ = main() catch return windows.FALSE;
    }

    return windows.TRUE;
}
