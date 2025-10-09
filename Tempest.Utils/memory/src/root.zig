const std = @import("std");
const windows = std.os.windows;
const builtin = @import("builtin");

extern "kernel32" fn GetModuleHandleA(lpModuleName: ?[*:0]const u8) callconv(.winapi) windows.HMODULE;

const MemError = error{
    InvalidAddress,
    InvalidHandle,
    InvalidModuleSize,
    InvalidModule,
    InvalidPattern,
    NoResult,
    OutOfBounds,
};

pub const Utils = struct {
    pub fn patternToBytes(allocator: std.mem.Allocator, patternStr: [*:0]const u8) ![]i16 {
        var bytes = std.ArrayList(i16).empty;
        errdefer bytes.deinit(allocator);

        var i: usize = 0;
        const len = std.mem.len(patternStr);

        while (i < len) {
            if (patternStr[i] == '?') {
                i += 1;
                if (i < len and patternStr[i] == '?') i += 1;
                try bytes.append(allocator, -1);
            } else if (patternStr[i] != ' ') {
                const remaining = patternStr[i..len];
                const byte = std.fmt.parseInt(u8, remaining[0..@min(2, remaining.len)], 16) catch {
                    i += 1;
                    continue;
                };
                try bytes.append(allocator, @as(i16, byte));
                i += if (byte > 0xF) @as(usize, 2) else 1;
            } else {
                i += 1;
            }
        }

        return bytes.toOwnedSlice(allocator);
    }
};

pub const Address = struct {
    address: usize,

    pub inline fn init(addr: usize) Address {
        return .{ .address = addr };
    }

    pub inline fn offset(self: *const Address, off: i32) Address {
        const newAddress = self.address +% @as(usize, @intCast(off));
        if (newAddress == 0)
            return MemError.InvalidAddress;

        return Address.init(newAddress);
    }

    pub inline fn relOffset(self: *const Address, off: i32) Address {
        const base = self.address +% @as(usize, @intCast(off));
        if (base == 0)
            return MemError.InvalidAddress;

        const displacement = @as(*const i32, @ptrFromInt(base)).*;
        return Address.init(base +% 4 +% @as(usize, @intCast(displacement)));
    }

    pub fn ptr(self: *const Address, comptime T: type) T {
        return @ptrFromInt(self.address);
    }

    pub inline fn get(self: *const Address) usize {
        return self.address;
    }
};

pub const Module = struct {
    handle: *anyopaque,
    sizeOfImage: usize,

    pub fn init(name: ?[*:0]const u8) !Module {
        const handle = GetModuleHandleA(name);

        return try Module.initFromHandle(handle);
    }

    pub fn initFromHandle(handle: *anyopaque) !Module {
        if (@intFromPtr(handle) == 0)
            return MemError.InvalidHandle;

        var module = Module{
            .handle = handle,
            .sizeOfImage = 0,
        };

        module.sizeOfImage = module.getSizeOfImage();

        if (module.sizeOfImage == 0 or module.sizeOfImage > 0x7FFFFFFF)
            return MemError.InvalidModuleSize;

        return module;
    }

    fn getSizeOfImage(self: *const Module) usize {
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

    pub fn getHandle(self: *const Module) Address {
        return .init(@intFromPtr(self.handle));
    }

    pub fn scanner(self: *const Module) Scanner {
        return Scanner.init(self);
    }

    pub const Scanner = struct {
        module: *const Module,
        address: ?Address = null,

        pub fn init(module: *const Module) Scanner {
            return .{ .module = module };
        }

        pub fn pattern(self: Scanner, allocator: std.mem.Allocator, patternStr: [*:0]const u8) ![]Address {
            if (std.mem.len(patternStr) == 0)
                return MemError.InvalidPattern;

            const patternBytes = try Utils.patternToBytes(allocator, patternStr);
            defer allocator.free(patternBytes);

            if (patternBytes.len == 0)
                return MemError.InvalidPattern;

            const handle = self.module.handle;
            const sizeOfImage = self.module.sizeOfImage;

            if (sizeOfImage == 0)
                return MemError.InvalidModuleSize;

            if (patternBytes.len > sizeOfImage)
                return MemError.OutOfBounds;

            const scanBytes = @as([*]const u8, @ptrCast(handle));
            const end = sizeOfImage - patternBytes.len;

            var results = std.ArrayList(Address).empty;
            errdefer results.deinit(allocator);

            var i: usize = 0;
            while (i <= end) : (i += 1) {
                var found = true;

                for (patternBytes, 0..) |byte, j| {
                    if (byte != -1 and scanBytes[i + j] != @as(u8, @intCast(byte))) {
                        found = false;
                        break;
                    }
                }

                if (found) {
                    const address = Address.init(@intFromPtr(&scanBytes[i]));
                    try results.append(allocator, address);
                }
            }

            if (results.items.len == 0) {
                return MemError.NoResult;
            }

            return results.toOwnedSlice(allocator);
        }

        pub fn string(self: *const Scanner, stringRef: []const u8) !Scanner {
            _ = self;
            _ = stringRef;
            return Scanner{};
        }
    };
};
