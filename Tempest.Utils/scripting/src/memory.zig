const std = @import("std");
const windows = std.os.windows;

extern "kernel32" fn GetModuleHandleA(lpModuleName: [*:?0]const u8) callconv(.winapi) windows.HMODULE;

pub const Address = struct {
    address: usize,

    pub inline fn init(addr: usize) Address {
        return .{ .address = addr };
    }

    pub inline fn offset(self: *const Address, off: i32) Address {
        return Address.init(self.address +% off);
    }

    pub inline fn relOffset(self: *const Address, off: i32) Address {
        const base = self.address +% @as(usize, @intCast(off));
        const displacement = @as(*const i32, @ptrFromInt(base)).*;
        return Address.init(base +% 4 +% @as(usize, @intCast(displacement)));
    }

    pub fn as(self: *const Address, comptime T: type) T {
        return @as(T, @ptrFromInt(self.address));
    }

    pub inline fn get(self: *const Address) usize {
        return self.address;
    }
};

pub const Module = struct {
    handle: *anyopaque,
    sizeOfImage: usize,

    pub fn init(name: [*:?0]const u8) !Module {
        const handle = GetModuleHandleA(name);

        return try Module.initFromHandle(handle);
    }

    pub fn initFromHandle(handle: *const anyopaque) !Module {
        // to-do: add safety checks

        var module = Module{
            .handle = handle,
            .sizeOfImage = 0,
        };

        module.sizeOfImage = module.getSizeOfImage();

        return module;
    }

    fn getSizeOfImage(self: *const Module) usize {
        const builtin = @import("builtin");
        const dos_header = self.handle;

        // dos_header->e_lfanew
        const e_lfanew = @as(*u32, @ptrFromInt(@intFromPtr(dos_header) + 0x3C)).*;
        const nt_headers = @intFromPtr(dos_header) + e_lfanew;

        const offset = switch (builtin.cpu.arch) {
            .x86 => 0x50,
            .x86_64 => 0x60,
            else => @compileError("Unsupported architecture"),
        };

        return @as(*u32, @ptrFromInt(nt_headers + offset)).*;
    }

    pub fn isAddressValid(self: *const Module, addr: usize) bool {
        return addr > self.handle and addr < self.handle + self.sizeOfImage;
    }

    pub fn get_handle(self: *const Module) Address {
        return .init(@intFromPtr(self.handle));
    }
};
