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

const StringEncoding = enum {
    utf8,
    utf16le,
    utf16be,
    utf32le,
    utf32be,
};

const NtHeaderType = switch (builtin.cpu.arch) {
    .x86 => ntapi.IMAGE_NT_HEADERS32,
    .x86_64 => ntapi.IMAGE_NT_HEADERS64,
    else => @compileError("Unsupported architecture"),
};

const Mnemonic = enum(u8) {
    PUSH = 0x68,
    JMP_REL8 = 0xEB,
    JMP_REL32 = 0xE9,
    JMP_EAX = 0xE0,
    CALL = 0xE8,
    LEA = 0x8D,
    CDQ = 0x99,
    CMOVL = 0x4C,
    CMOVS = 0x48,
    CMOVNS = 0x49,
    NOP = 0x90,
    INT3 = 0xCC,
    RETN_REL8 = 0xC2,
    RETN = 0xC3,
    NONE = 0x00,
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

    // NOTE: got lazy so claude did this, there might be a better way to do this
    fn matchString(stringBytes: [*]align(1) const u8, needle: []const u8, section: anytype) !bool {
        if (matchUtf8(stringBytes, needle, section)) {
            return true;
        }

        if (matchUtf16Le(stringBytes, needle, section)) {
            return true;
        }

        if (matchUtf16Be(stringBytes, needle, section)) {
            return true;
        }

        if (matchUtf32Le(stringBytes, needle, section)) {
            return true;
        }

        if (matchUtf32Be(stringBytes, needle, section)) {
            return true;
        }

        return false;
    }

    fn matchUtf8(stringBytes: [*]align(1) const u8, needle: []const u8, section: anytype) bool {
        const firstByte = stringBytes[0];
        if (firstByte == 0 or firstByte > 0x7F) {
            if (firstByte == 0) return false;
        }

        var len: usize = 0;
        const maxLen = section.size;
        while (len < maxLen and len < 4096) : (len += 1) {
            if (stringBytes[len] == 0) break;
        }

        if (len == 0 or len >= 4096) return false;
        if (len != needle.len) return false;

        for (needle, 0..) |byte, idx| {
            if (stringBytes[idx] != byte) return false;
        }

        return true;
    }

    fn matchUtf16Le(stringBytes: [*]align(1) const u8, needle: []const u8, section: anytype) bool {
        const firstChar = stringBytes[0];
        const secondByte = stringBytes[1];

        if (firstChar == 0 or (firstChar > 0x7F and secondByte != 0)) {
            return false;
        }

        var len: usize = 0;
        const maxLen = section.size / 2;
        while (len < maxLen and len < 2048) : (len += 1) {
            const idx = len * 2;
            if (stringBytes[idx] == 0 and stringBytes[idx + 1] == 0) break;
        }

        if (len == 0 or len >= 2048) return false;
        if (len != needle.len) return false;

        for (needle, 0..) |byte, idx| {
            const charIdx = idx * 2;
            const char = stringBytes[charIdx];
            const highByte = stringBytes[charIdx + 1];

            if (char != byte or highByte != 0) return false;
        }

        return true;
    }

    fn matchUtf16Be(stringBytes: [*]align(1) const u8, needle: []const u8, section: anytype) bool {
        const firstByte = stringBytes[0];
        const secondByte = stringBytes[1];

        if (firstByte != 0 or secondByte == 0) {
            return false;
        }

        var len: usize = 0;
        const maxLen = section.size / 2;
        while (len < maxLen and len < 2048) : (len += 1) {
            const idx = len * 2;
            if (stringBytes[idx] == 0 and stringBytes[idx + 1] == 0) break;
        }

        if (len == 0 or len >= 2048) return false;
        if (len != needle.len) return false;

        for (needle, 0..) |byte, idx| {
            const charIdx = idx * 2;
            const highByte = stringBytes[charIdx];
            const char = stringBytes[charIdx + 1];

            if (char != byte or highByte != 0) return false;
        }

        return true;
    }

    fn matchUtf32Le(stringBytes: [*]align(1) const u8, needle: []const u8, section: anytype) bool {
        if (stringBytes[1] != 0 or stringBytes[2] != 0 or stringBytes[3] != 0) {
            return false;
        }

        var len: usize = 0;
        const maxLen = section.size / 4;
        while (len < maxLen and len < 1024) : (len += 1) {
            const idx = len * 4;
            if (stringBytes[idx] == 0 and stringBytes[idx + 1] == 0 and
                stringBytes[idx + 2] == 0 and stringBytes[idx + 3] == 0) break;
        }

        if (len == 0 or len >= 1024) return false;
        if (len != needle.len) return false;

        for (needle, 0..) |byte, idx| {
            const charIdx = idx * 4;
            if (stringBytes[charIdx] != byte or
                stringBytes[charIdx + 1] != 0 or
                stringBytes[charIdx + 2] != 0 or
                stringBytes[charIdx + 3] != 0) return false;
        }

        return true;
    }

    fn matchUtf32Be(stringBytes: [*]align(1) const u8, needle: []const u8, section: anytype) bool {
        if (stringBytes[0] != 0 or stringBytes[1] != 0 or stringBytes[2] != 0) {
            return false;
        }

        var len: usize = 0;
        const maxLen = section.size / 4;
        while (len < maxLen and len < 1024) : (len += 1) {
            const idx = len * 4;
            if (stringBytes[idx] == 0 and stringBytes[idx + 1] == 0 and
                stringBytes[idx + 2] == 0 and stringBytes[idx + 3] == 0) break;
        }

        if (len == 0 or len >= 1024) return false;
        if (len != needle.len) return false;

        for (needle, 0..) |byte, idx| {
            const charIdx = idx * 4;
            if (stringBytes[charIdx] != 0 or
                stringBytes[charIdx + 1] != 0 or
                stringBytes[charIdx + 2] != 0 or
                stringBytes[charIdx + 3] != byte) return false;
        }

        return true;
    }
};

pub const Address = struct {
    address: usize,

    pub inline fn init(addr: usize) Address {
        return .{ .address = addr };
    }

    pub inline fn absoluteOffset(self: *const Address, off: usize) !Address {
        const base = self.address +% off;
        if (base == 0)
            return MemError.InvalidAddress;

        const address = @as(*align(1) const u32, @ptrFromInt(base)).*;
        // const address = @as(*const u32, @ptrFromInt(base)).*;
        if (address == 0)
            return MemError.InvalidAddress;

        return Address.init(address);
    }

    pub inline fn relativeOffset(self: *const Address, off: usize) !Address {
        const base = self.address +% off;
        if (base == 0)
            return MemError.InvalidAddress;

        const displacement = @as(*const u32, @ptrFromInt(base)).*;
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
        return addr > @as(usize, @intFromPtr(self.handle)) and addr < @as(usize, @intFromPtr(self.handle)) + self.sizeOfImage;
    }

    pub fn getHandle(self: *const Module) Address {
        return .init(@intFromPtr(self.handle));
    }

    pub fn section(self: *const Module, allocator: std.mem.Allocator, name: []const u8) !Section {
        const sec = Section{
            .module = self,
        };
        return try .init(sec, allocator, name);
    }

    pub const Section = struct {
        module: *const Module,
        name: []const u8 = "",
        size: usize = 0,
        start: ?Address = null,
        end: ?Address = null,

        pub fn init(sec: Section, allocator: std.mem.Allocator, name: []const u8) !Section {
            const s = try sec.getSection(allocator, name);
            const start = sec.module.getHandle().get() + s.VirtualAddress;

            return .{
                .module = sec.module,
                .name = name,
                .size = s.VirtualSize,
                .start = Address{ .address = start },
                .end = Address{ .address = start + s.VirtualSize },
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

        pub fn isInSection(self: Section, address: Address) bool {
            return address.get() >= self.start.?.get() and address.get() < self.end.?.get();
        }
    };

    const StringRef = struct {
        address: Address,
        dataAddress: Address,
        encoding: StringEncoding,
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

        // TODO: add x64 support, fuzzy matching
        pub fn string(self: Scanner, allocator: std.mem.Allocator, comptime str: []const u8, findFirst: bool) ![]StringRef {
            var addresses = std.ArrayList(StringRef).empty;
            errdefer addresses.deinit(allocator);

            const textSection = try self.module.section(std.heap.page_allocator, ".text");
            const rdataSection = try self.module.section(std.heap.page_allocator, ".rdata");

            const scanBytes = textSection.start.?.ptr([*]const u8);

            // NOTE: comments are for x64, need to implement later
            var i: usize = 0;
            while (i < textSection.size) : (i += 1) {
                // if ((scanBytes[i] == @intFromEnum(Mnemonic.CMOVL) or
                //     scanBytes[i] == @intFromEnum(Mnemonic.CMOVS) or
                //     scanBytes[i] == @intFromEnum(Mnemonic.CMOVNS)) and
                //     scanBytes[i + 1] == @intFromEnum(Mnemonic.LEA))
                // {
                if (scanBytes[i] == @intFromEnum(Mnemonic.PUSH)) {
                    // const stringAddress = try Address.init(@intFromPtr(&scanBytes[i])).relativeOffset(3);
                    const stringAddress = Address.init(@intFromPtr(&scanBytes[i])).absoluteOffset(1) catch {
                        continue;
                    };

                    if (!self.module.isAddressValid(stringAddress.address)) {
                        continue;
                    }

                    if (rdataSection.isInSection(stringAddress)) {
                        const stringBytes = @as([*]align(1) const u8, @ptrFromInt(stringAddress.address));

                        if (try Utils.matchString(stringBytes, str, rdataSection)) {
                            if (findFirst) {
                                try addresses.append(allocator, .{
                                    .address = Address.init(@intFromPtr(&scanBytes[i])),
                                    .dataAddress = stringAddress,
                                    .encoding = .utf8, // TODO: don't hardcode this
                                });
                                return addresses.toOwnedSlice(allocator);
                            } else {
                                try addresses.append(allocator, .{
                                    .address = Address.init(@intFromPtr(&scanBytes[i])),
                                    .dataAddress = stringAddress,
                                    .encoding = .utf8, // TODO: don't hardcode this either
                                });
                            }
                        }
                    }
                }
            }

            if (addresses.items.len > 0) {
                return addresses.toOwnedSlice(allocator);
            }

            return MemError.NoResult;
        }
    };
};
