const std = @import("std");
const windows = std.os.windows;
const builtin = @import("builtin");

const ntapi = @import("ntapi.zig");

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

const NtHeaderType = switch (builtin.cpu.arch) {
    .x86 => ntapi.IMAGE_NT_HEADERS32,
    .x86_64 => ntapi.IMAGE_NT_HEADERS64,
    else => @compileError("Unsupported architecture"),
};

pub const Utils = struct {
    pub inline fn patternToBytes(comptime patternStr: []const u8) []const i16 {
        var bytes: [patternStr.len]i16 = undefined;
        var count: usize = 0;
        var i: usize = 0;

        while (i < patternStr.len) : (i += 1) {
            const c = patternStr[i];

            if (c == '?') {
                i += 1;
                if (i < patternStr.len and patternStr[i] == '?') i += 1;
                bytes[count] = -1;
                count += 1;
            } else if (c != ' ') {
                const remaining = patternStr[i..];
                const len = @min(2, remaining.len);
                const hex = remaining[0..len];

                // TODO: make this a compile time error
                const byte = std.fmt.parseInt(u8, hex, 16) catch {
                    continue;
                };

                bytes[count] = @as(i16, byte);
                count += 1;

                i += len - 1;
            }
        }

        return bytes[0..count];
    }
};

pub const Address = struct {
    address: usize,

    pub inline fn init(addr: usize) Address {
        return .{ .address = addr };
    }

    pub inline fn offset(self: *const Address, off: i32) !Address {
        const newAddress = self.address +% @as(usize, @intCast(off));
        if (newAddress == 0)
            return MemError.InvalidAddress;

        return Address.init(newAddress);
    }

    pub inline fn relOffset(self: *const Address, off: i32) !Address {
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

    fn getDosHeader(self: *const Module) *const ntapi.IMAGE_DOS_HEADER {
        return @as(*const ntapi.IMAGE_DOS_HEADER, @ptrCast(@alignCast(self.handle)));
    }

    fn getNtHeader(self: *const Module) *const NtHeaderType {
        const dos_header = self.getDosHeader();
        const nt_headers_addr = @intFromPtr(dos_header) + @as(usize, @intCast(dos_header.e_lfanew));
        return @ptrFromInt(nt_headers_addr);
    }

    fn getSizeOfImage(self: *const Module) usize {
        const nt_headers = self.getNtHeader();
        return nt_headers.OptionalHeader.SizeOfImage;
    }

    pub fn isAddressValid(self: *const Module, addr: usize) bool {
        return addr > self.handle and addr < self.handle + self.sizeOfImage;
    }

    pub fn getHandle(self: *const Module) Address {
        return .init(@intFromPtr(self.handle));
    }

    pub fn section(self: *const Module, allocator: std.mem.Allocator, name: []const u8) !Section {
        const sec = Section{
            .module = self,
            .name = "",
        };
        return try .init(sec, allocator, name);
    }

    pub const Section = struct {
        module: *const Module,
        name: []const u8,

        pub fn init(sec: Section, allocator: std.mem.Allocator, name: []const u8) !Section {
            _ = try sec.getSection(allocator, name);

            return .{
                .module = sec.module,
                .name = name,
            };
        }

        pub fn getAllSections(self: Section, allocator: std.mem.Allocator) ![]ntapi.IMAGE_SECTION_HEADER {
            const nt_headers = self.module.getNtHeader();
            const num_sections = nt_headers.FileHeader.NumberOfSections;

            var sections = std.ArrayList(ntapi.IMAGE_SECTION_HEADER).empty;
            errdefer sections.deinit(allocator);

            const nt_headers_addr = @intFromPtr(nt_headers);
            const optional_header_size = nt_headers.FileHeader.SizeOfOptionalHeader;

            const first_section_addr = nt_headers_addr + 4 + 20 + optional_header_size;
            const section_headers = @as([*]const ntapi.IMAGE_SECTION_HEADER, @ptrFromInt(first_section_addr));

            for (0..num_sections) |i| {
                try sections.append(allocator, section_headers[i]);
            }

            return sections.toOwnedSlice(allocator);
        }

        pub fn getSection(self: Section, allocator: std.mem.Allocator, name: []const u8) !ntapi.IMAGE_SECTION_HEADER {
            const sections = try self.getAllSections(allocator);
            defer allocator.free(sections);

            for (sections) |s| {
                if (std.mem.eql(u8, s.getName(), name)) {
                    return s;
                }
            }

            return MemError.NoResult;
        }
    };

    pub fn scanner(self: *const Module) Scanner {
        return Scanner.init(self);
    }

    pub const Scanner = struct {
        module: *const Module,
        address: ?Address = null,

        pub fn init(module: *const Module) Scanner {
            return .{ .module = module };
        }

        pub fn pattern(self: Scanner, allocator: std.mem.Allocator, comptime patternStr: []const u8) ![]Address {
            if (patternStr.len == 0) @compileError("Pattern string must not be empty");
            const patternBytes = Utils.patternToBytes(patternStr);

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

        // TODO: implement
        pub fn string(self: *const Scanner, stringRef: []const u8) !Scanner {
            _ = self;
            _ = stringRef;
            return Scanner{};
        }
    };
};
