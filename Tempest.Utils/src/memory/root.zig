const std = @import("std");
const windows = std.os.windows;
const builtin = @import("builtin");

const zydis = @import("zydis");

const ntapi = @import("ntapi.zig");

extern "kernel32" fn GetModuleHandleA(lpModuleName: ?[*:0]const u8) callconv(.winapi) windows.HMODULE;

pub const MemError = error{
    InvalidAddress,
    InvalidHandle,
    InvalidModuleSize,
    InvalidModule,
    InvalidPattern,
    NoResult,
    OutOfBounds,
    DecoderInitFailed,
    DecodeFailed,
};

const NtHeaderType = switch (builtin.cpu.arch) {
    .x86 => ntapi.IMAGE_NT_HEADERS32,
    .x86_64 => ntapi.IMAGE_NT_HEADERS64,
    else => @compileError("Unsupported architecture"),
};

// Comptime decoder constants — used everywhere instead of hardcoded x64 values.
const MACHINE_MODE: c_uint = switch (builtin.cpu.arch) {
    .x86 => zydis.ZYDIS_MACHINE_MODE_LEGACY_32,
    .x86_64 => zydis.ZYDIS_MACHINE_MODE_LONG_64,
    else => @compileError("Unsupported architecture"),
};

const STACK_WIDTH: c_uint = switch (builtin.cpu.arch) {
    .x86 => zydis.ZYDIS_STACK_WIDTH_32,
    .x86_64 => zydis.ZYDIS_STACK_WIDTH_64,
    else => @compileError("Unsupported architecture"),
};

// On x86 there is no RIP — the base register for RIP-relative is NONE (absolute disp).
// On x86 LEA [disp32] has base == ZYDIS_REGISTER_NONE and no index; we match on that.
const IS_X64 = builtin.cpu.arch == .x86_64;

fn makeDecoder() !zydis.ZydisDecoder {
    var decoder: zydis.ZydisDecoder = undefined;
    if (zydis.ZydisDecoderInit(&decoder, MACHINE_MODE, STACK_WIDTH) != zydis.ZYAN_STATUS_SUCCESS) {
        return MemError.DecoderInitFailed;
    }
    return decoder;
}

const Utils = struct {
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

    fn readInstructionBytes(address: usize, buffer: []u8) usize {
        const src: [*]const u8 = @ptrFromInt(address);
        @memcpy(buffer, src[0..buffer.len]);
        return buffer.len;
    }
};

pub const Address = struct {
    address: usize,

    pub fn init(addr: usize) Address {
        return .{ .address = addr };
    }

    pub fn absolute(self: Address, off: usize) !Address {
        const base = self.address + off;
        const value = @as(*align(1) const u32, @ptrFromInt(base)).*;
        return Address.init(@as(usize, value));
    }

    pub fn relative(self: Address, off: usize) !Address {
        const base = self.address + off;
        const disp = @as(*align(1) const u32, @ptrFromInt(base)).*;
        const signed = @as(i32, @bitCast(disp));
        const target_signed = @as(isize, @intCast(base + 4)) + @as(isize, signed);
        return Address.init(@intCast(@as(usize, @intCast(target_signed))));
    }

    pub fn toRva(self: Address, module: *const Module) usize {
        return self.address - module.base_address;
    }

    pub fn toRvaOffset(self: Address, module: *const Module, off: usize) usize {
        return off + (self.address - module.base_address);
    }

    pub fn fromRva(rva: usize, module: *const Module) Address {
        return Address.init(module.base_address + rva);
    }

    pub fn fromRvaOffset(rva: usize, module: *const Module, off: usize) Address {
        return Address.init(module.base_address + rva + off);
    }

    /// Resolves the effective target address of the instruction at this address.
    /// Handles RIP-relative (x64) and absolute displacement (x86) memory operands,
    /// plus relative branch immediates on both architectures.
    pub fn getAddress(self: Address) !Address {
        var decoder = try makeDecoder();

        var buffer: [32]u8 = undefined;
        _ = Utils.readInstructionBytes(self.address, &buffer);

        var inst: zydis.ZydisDecodedInstruction = undefined;
        var operands: [zydis.ZYDIS_MAX_OPERAND_COUNT]zydis.ZydisDecodedOperand = undefined;

        if (zydis.ZydisDecoderDecodeFull(&decoder, &buffer, buffer.len, &inst, &operands) != zydis.ZYAN_STATUS_SUCCESS) {
            return MemError.DecodeFailed;
        }

        // Relative branch (works the same on x86 and x64)
        if (inst.meta.branch_type != zydis.ZYDIS_BRANCH_TYPE_NONE) {
            if (inst.operand_count > 0 and operands[0].type == zydis.ZYDIS_OPERAND_TYPE_IMMEDIATE) {
                const imm_op = operands[0];
                if (imm_op.unnamed_0.imm.is_relative != 0) {
                    const ip_after = self.address + inst.length;
                    const displacement = @as(i64, @bitCast(imm_op.unnamed_0.imm.value.s));
                    const target = if (displacement >= 0)
                        ip_after + @as(usize, @intCast(displacement))
                    else
                        ip_after - @as(usize, @intCast(-displacement));
                    return Address.init(target);
                }
            }
        }

        // Memory operand: RIP-relative on x64, absolute disp on x86
        if (inst.operand_count >= 2) {
            const mem_op = operands[1];
            if (mem_op.type == zydis.ZYDIS_OPERAND_TYPE_MEMORY) {
                if (IS_X64) {
                    // x64: base register must be RIP
                    if (mem_op.unnamed_0.mem.base == zydis.ZYDIS_REGISTER_RIP) {
                        const rip_after = self.address + inst.length;
                        const disp = mem_op.unnamed_0.mem.disp.value;
                        const signed_disp = @as(i64, @bitCast(@as(u64, @intCast(disp))));
                        const target = if (signed_disp >= 0)
                            rip_after + @as(usize, @intCast(signed_disp))
                        else
                            rip_after - @as(usize, @intCast(-signed_disp));
                        return Address.init(target);
                    }
                } else {
                    // x86: base == NONE with a displacement is a direct memory address
                    if (mem_op.unnamed_0.mem.base == zydis.ZYDIS_REGISTER_NONE and
                        mem_op.unnamed_0.mem.disp.has_displacement != 0)
                    {
                        return Address.init(@as(usize, @intCast(
                            @as(u32, @bitCast(@as(i32, @intCast(mem_op.unnamed_0.mem.disp.value)))),
                        )));
                    }
                }
            }
        }

        return MemError.NoResult;
    }

    /// Returns the immediate value of the given operand index at this address.
    pub fn getData(self: Address, operand_index: usize) !usize {
        var decoder = try makeDecoder();

        var buffer: [32]u8 = undefined;
        _ = Utils.readInstructionBytes(self.address, &buffer);

        var inst: zydis.ZydisDecodedInstruction = undefined;
        var operands: [zydis.ZYDIS_MAX_OPERAND_COUNT]zydis.ZydisDecodedOperand = undefined;

        if (zydis.ZydisDecoderDecodeFull(&decoder, &buffer, buffer.len, &inst, &operands) != zydis.ZYAN_STATUS_SUCCESS) {
            return MemError.DecodeFailed;
        }

        if (operand_index < inst.operand_count) {
            const op = operands[operand_index];
            if (op.type == zydis.ZYDIS_OPERAND_TYPE_IMMEDIATE) {
                return @intCast(op.unnamed_0.imm.value.u);
            }
        }

        return 0;
    }

    /// Returns the memory displacement of the given operand index at this address.
    pub fn getDisplacement(self: Address, operand_index: usize) !usize {
        var decoder = try makeDecoder();

        var buffer: [32]u8 = undefined;
        _ = Utils.readInstructionBytes(self.address, &buffer);

        var inst: zydis.ZydisDecodedInstruction = undefined;
        var operands: [zydis.ZYDIS_MAX_OPERAND_COUNT]zydis.ZydisDecodedOperand = undefined;

        if (zydis.ZydisDecoderDecodeFull(&decoder, &buffer, buffer.len, &inst, &operands) != zydis.ZYAN_STATUS_SUCCESS) {
            return MemError.DecodeFailed;
        }

        if (operand_index < inst.operand_count) {
            const op = operands[operand_index];
            if (op.type == zydis.ZYDIS_OPERAND_TYPE_MEMORY and op.unnamed_0.mem.disp.has_displacement != 0) {
                return @as(usize, @intCast(@as(u32, @bitCast(@as(i32, @intCast(op.unnamed_0.mem.disp.value))))));
            }
        }

        return 0;
    }

    pub fn AsPtr(self: Address, comptime T: type) T {
        return @ptrFromInt(self.address);
    }

    pub fn get(self: Address) usize {
        return self.address;
    }
};

pub const Module = struct {
    handle: *anyopaque,
    base_address: usize,
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
            .base_address = @intFromPtr(handle),
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
        return self.getNtHeader().OptionalHeader.SizeOfImage;
    }

    pub fn isAddressValid(self: *const Module, addr: usize) bool {
        return addr >= self.base_address and addr < self.base_address + self.sizeOfImage;
    }

    pub fn getHandle(self: *const Module) Address {
        return .init(self.base_address);
    }

    pub fn section(self: *const Module, name: []const u8) !Section {
        return Section.init(self, name);
    }

    pub const Section = struct {
        module: *const Module,
        name: []const u8 = "",
        size: usize = 0,
        va: ?Address = null,
        start: ?Address = null,
        end: ?Address = null,

        pub fn init(module: *const Module, name: []const u8) !Section {
            const s = try getSectionHeader(module, name);
            const start = module.base_address + s.VirtualAddress;

            return .{
                .module = module,
                .name = name,
                .size = s.VirtualSize,
                .va = Address{ .address = s.VirtualAddress },
                .start = Address{ .address = start },
                .end = Address{ .address = start + s.VirtualSize },
            };
        }

        fn getSectionHeadersPtr(module: *const Module) [*]const ntapi.IMAGE_SECTION_HEADER {
            const nt_headers = module.getNtHeader();
            const nt_headers_addr = @intFromPtr(nt_headers);
            const optional_header_size = nt_headers.FileHeader.SizeOfOptionalHeader;
            const first_section_addr = nt_headers_addr + 4 + 20 + optional_header_size;
            return @as([*]const ntapi.IMAGE_SECTION_HEADER, @ptrFromInt(first_section_addr));
        }

        fn getSectionHeader(module: *const Module, name: []const u8) !ntapi.IMAGE_SECTION_HEADER {
            const section_headers = getSectionHeadersPtr(module);
            const num_sections = module.getNtHeader().FileHeader.NumberOfSections;

            for (0..num_sections) |i| {
                if (std.mem.eql(u8, section_headers[i].getName(), name)) {
                    return section_headers[i];
                }
            }

            return MemError.NoResult;
        }

        pub fn isInSection(self: Section, address: Address) bool {
            return address.get() >= self.start.?.get() and address.get() < self.end.?.get();
        }
    };

    pub fn scanner(self: *const Module) Scanner {
        return Scanner.init(self);
    }

    pub const Scanner = struct {
        module: *const Module,
        address: ?Address = null,
        dataAddress: ?Address = null,

        pub fn init(module: *const Module) Scanner {
            return .{ .module = module };
        }

        pub fn pattern(self: Scanner, allocator: std.mem.Allocator, comptime patternStr: []const u8) ![]Address {
            if (patternStr.len == 0) @compileError("Pattern string must not be empty");
            const patternBytes = Utils.patternToBytes(patternStr);

            if (patternBytes.len == 0)
                return MemError.InvalidPattern;

            if (patternBytes.len > self.module.sizeOfImage)
                return MemError.OutOfBounds;

            const scanBytes = @as([*]const u8, @ptrCast(self.module.handle));
            const end = self.module.sizeOfImage - patternBytes.len;

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
                    try results.append(allocator, Address.init(@intFromPtr(&scanBytes[i])));
                }
            }

            if (results.items.len == 0) return MemError.NoResult;

            return results.toOwnedSlice(allocator);
        }

        /// Finds all LEA instructions in .text that reference the given string in .rdata,
        /// using Zydis to decode operands precisely.
        /// Works on both x86 (EIP-relative via absolute disp) and x64 (RIP-relative).
        pub fn string(self: Scanner, allocator: std.mem.Allocator, comptime str: []const u8) ![]Scanner {
            const text_section = try self.module.section(".text");
            const rdata_section = try self.module.section(".rdata");

            const rdata_bytes = rdata_section.start.?.AsPtr([*]const u8);

            const utf16_str = comptime blk: {
                var buf: [str.len * 2]u8 = undefined;
                for (str, 0..) |c, i| {
                    buf[i * 2] = c;
                    buf[i * 2 + 1] = 0;
                }
                break :blk buf;
            };

            // Find UTF-8 occurrence
            var utf8_addr: ?usize = null;
            for (0..rdata_section.size - str.len) |offset| {
                if (std.mem.eql(u8, rdata_bytes[offset..][0..str.len], str)) {
                    utf8_addr = rdata_section.start.?.get() + offset;
                    break;
                }
            }

            // Find UTF-16LE occurrence
            var utf16_addr: ?usize = null;
            if (rdata_section.size >= utf16_str.len) {
                for (0..rdata_section.size - utf16_str.len) |offset| {
                    if (std.mem.eql(u8, rdata_bytes[offset..][0..utf16_str.len], &utf16_str)) {
                        utf16_addr = rdata_section.start.?.get() + offset;
                        break;
                    }
                }
            }

            std.log.debug("[Scanner.string] query=\"{s}\"", .{str});
            if (utf8_addr) |a| {
                std.log.debug("[Scanner.string] utf8  found @ 0x{x}", .{a});
            } else {
                std.log.debug("[Scanner.string] utf8  not found in .rdata", .{});
            }
            if (utf16_addr) |a| {
                std.log.debug("[Scanner.string] utf16 found @ 0x{x}", .{a});
            } else {
                std.log.debug("[Scanner.string] utf16 not found in .rdata", .{});
            }

            if (utf8_addr == null and utf16_addr == null) return MemError.NoResult;

            var decoder = try makeDecoder();
            var results = std.ArrayList(Scanner).empty;
            errdefer results.deinit(allocator);

            const scan_bytes = text_section.start.?.AsPtr([*]const u8);
            var buffer: [32]u8 = undefined;
            var i: usize = 0;

            while (i < text_section.size - 7) {
                const byte = scan_bytes[i];

                // LEA: x64 REX + 0x8D, x86 0x8D
                const is_lea_candidate = if (IS_X64)
                    ((byte & 0xF0) == 0x40 and scan_bytes[i + 1] == 0x8D)
                else
                    (byte == 0x8D);

                // PUSH imm32: x86 only, 0x68
                const is_push_candidate = !IS_X64 and byte == 0x68;

                if (is_lea_candidate or is_push_candidate) {
                    const bytes_to_read = @min(buffer.len, text_section.size - i);
                    @memcpy(buffer[0..bytes_to_read], scan_bytes[i..][0..bytes_to_read]);

                    var inst: zydis.ZydisDecodedInstruction = undefined;
                    var operands: [zydis.ZYDIS_MAX_OPERAND_COUNT]zydis.ZydisDecodedOperand = undefined;

                    const status = zydis.ZydisDecoderDecodeFull(&decoder, &buffer, bytes_to_read, &inst, &operands);

                    if (status == zydis.ZYAN_STATUS_SUCCESS and
                        (inst.mnemonic == zydis.ZYDIS_MNEMONIC_LEA or
                            inst.mnemonic == zydis.ZYDIS_MNEMONIC_PUSH) and
                        inst.operand_count >= 1)
                    {
                        const current_addr = text_section.start.?.get() + i;

                        const resolved_addr: ?usize = if (inst.mnemonic == zydis.ZYDIS_MNEMONIC_LEA) blk: {
                            // LEA uses operand[1] (dst, src)
                            const mem_op = operands[1];
                            if (IS_X64) {
                                // x64: RIP-relative
                                if (mem_op.type == zydis.ZYDIS_OPERAND_TYPE_MEMORY and
                                    mem_op.unnamed_0.mem.base == zydis.ZYDIS_REGISTER_RIP)
                                {
                                    const disp: i32 = @intCast(mem_op.unnamed_0.mem.disp.value);
                                    const rip_after = current_addr + inst.length;
                                    break :blk if (disp >= 0)
                                        rip_after + @as(usize, @intCast(disp))
                                    else
                                        rip_after - @as(usize, @intCast(-disp));
                                }
                            } else {
                                // x86: absolute displacement
                                if (mem_op.type == zydis.ZYDIS_OPERAND_TYPE_MEMORY and
                                    mem_op.unnamed_0.mem.base == zydis.ZYDIS_REGISTER_NONE and
                                    mem_op.unnamed_0.mem.disp.has_displacement != 0)
                                {
                                    break :blk @as(usize, @intCast(
                                        @as(u32, @bitCast(@as(i32, @intCast(mem_op.unnamed_0.mem.disp.value)))),
                                    ));
                                }
                            }
                            break :blk null;
                        } else blk: {
                            // PUSH imm32 uses operand[0]
                            const imm_op = operands[0];
                            if (imm_op.type == zydis.ZYDIS_OPERAND_TYPE_IMMEDIATE) {
                                break :blk @as(usize, @intCast(@as(u32, @truncate(
                                    @as(u64, @bitCast(imm_op.unnamed_0.imm.value)),
                                ))));
                            }
                            break :blk null;
                        };

                        if (resolved_addr) |addr| {
                            const is_utf8_hit = utf8_addr != null and addr == utf8_addr.?;
                            const is_utf16_hit = utf16_addr != null and addr == utf16_addr.?;
                            if (is_utf8_hit or is_utf16_hit) {
                                std.log.debug("[Scanner.string] match @ 0x{x} -> 0x{x} ({s}, {s})", .{
                                    current_addr,
                                    addr,
                                    if (is_utf16_hit) "utf16" else "utf8",
                                    if (inst.mnemonic == zydis.ZYDIS_MNEMONIC_PUSH) "push" else "lea",
                                });
                                try results.append(allocator, Scanner{
                                    .module = self.module,
                                    .address = Address.init(current_addr),
                                    .dataAddress = Address.init(addr),
                                });
                            }
                        }

                        i += if (inst.length > 0) inst.length else 1;
                        continue;
                    }
                }

                i += 1;
            }

            std.log.debug("[Scanner.string] total matches: {d}", .{results.items.len});

            if (results.items.len == 0) return MemError.NoResult;

            return results.toOwnedSlice(allocator);
        }

        pub const FindNextOptions = struct {
            inst: ?zydis.ZydisMnemonic = null,
            instructions: ?[]const zydis.ZydisMnemonic = null,
            dst_reg: ?zydis.ZydisRegister = null,
            src_reg: ?zydis.ZydisRegister = null,
            dst_width: ?u16 = null,
            src_width: ?u16 = null,
            dst_disp: ?bool = null,
            src_disp: ?bool = null,
            skip: usize = 0,
            limit: ?usize = null,
        };

        pub fn findNext(self: Scanner, start: usize, options: FindNextOptions) !Address {
            var decoder = try makeDecoder();

            var buffer: [32]u8 = undefined;
            var current = start;
            var skip_remaining = options.skip;
            var instructions_checked: usize = 0;

            const needs_operands = options.dst_reg != null or options.src_reg != null or
                options.src_width != null or options.dst_width != null or
                options.dst_disp != null or options.src_disp != null;

            while (true) {
                if (options.limit) |limit| {
                    if (instructions_checked >= limit) return MemError.NoResult;
                }

                _ = Utils.readInstructionBytes(current, &buffer);

                var inst: zydis.ZydisDecodedInstruction = undefined;
                var operands: [zydis.ZYDIS_MAX_OPERAND_COUNT]zydis.ZydisDecodedOperand = undefined;

                const status = if (needs_operands)
                    zydis.ZydisDecoderDecodeFull(&decoder, &buffer, buffer.len, &inst, &operands)
                else
                    zydis.ZydisDecoderDecodeInstruction(&decoder, null, &buffer, buffer.len, &inst);

                if (status == zydis.ZYAN_STATUS_SUCCESS) {
                    instructions_checked += 1;

                    const inst_matches = blk: {
                        if (options.instructions) |insts| {
                            for (insts) |target_inst| {
                                if (inst.mnemonic == target_inst) break :blk true;
                            }
                            break :blk false;
                        } else if (options.inst) |target_inst| {
                            break :blk inst.mnemonic == target_inst;
                        } else {
                            break :blk false;
                        }
                    };

                    if (inst_matches) {
                        var matches = true;

                        if (options.dst_width) |width| {
                            if (inst.operand_count < 1) {
                                matches = false;
                            } else {
                                const op = operands[0];
                                switch (op.type) {
                                    zydis.ZYDIS_OPERAND_TYPE_REGISTER => {
                                        const reg_width = zydis.ZydisRegisterGetWidth(decoder.machine_mode, op.unnamed_0.reg.value);
                                        if (reg_width != width) matches = false;
                                    },
                                    zydis.ZYDIS_OPERAND_TYPE_MEMORY => {
                                        if (op.unnamed_0.mem.base != zydis.ZYDIS_REGISTER_NONE) {
                                            const reg_width = zydis.ZydisRegisterGetWidth(decoder.machine_mode, op.unnamed_0.mem.base);
                                            if (reg_width != width) matches = false;
                                        } else {
                                            matches = false;
                                        }
                                    },
                                    else => matches = false,
                                }
                            }
                        }

                        if (options.src_width) |width| {
                            if (inst.operand_count < 2) {
                                matches = false;
                            } else {
                                const op = operands[1];
                                switch (op.type) {
                                    zydis.ZYDIS_OPERAND_TYPE_REGISTER => {
                                        const reg_width = zydis.ZydisRegisterGetWidth(decoder.machine_mode, op.unnamed_0.reg.value);
                                        if (reg_width != width) matches = false;
                                    },
                                    zydis.ZYDIS_OPERAND_TYPE_MEMORY => {
                                        if (op.unnamed_0.mem.base != zydis.ZYDIS_REGISTER_NONE) {
                                            const reg_width = zydis.ZydisRegisterGetWidth(decoder.machine_mode, op.unnamed_0.mem.base);
                                            if (reg_width != width) matches = false;
                                        } else {
                                            matches = false;
                                        }
                                    },
                                    else => matches = false,
                                }
                            }
                        }

                        if (options.dst_reg) |r_dst| {
                            if (inst.operand_count == 0) {
                                matches = false;
                            } else {
                                const op = operands[0];
                                const found_reg = (op.type == zydis.ZYDIS_OPERAND_TYPE_REGISTER and op.unnamed_0.reg.value == r_dst) or
                                    (op.type == zydis.ZYDIS_OPERAND_TYPE_MEMORY and op.unnamed_0.mem.base == r_dst);
                                if (!found_reg) matches = false;
                            }
                        }

                        if (options.src_reg) |r_src| {
                            if (inst.operand_count < 2) {
                                matches = false;
                            } else {
                                const op = operands[1];
                                const found_reg = (op.type == zydis.ZYDIS_OPERAND_TYPE_REGISTER and op.unnamed_0.reg.value == r_src) or
                                    (op.type == zydis.ZYDIS_OPERAND_TYPE_MEMORY and op.unnamed_0.mem.base == r_src);
                                if (!found_reg) matches = false;
                            }
                        }

                        if (options.dst_disp) |requires_disp| {
                            if (inst.operand_count < 1) {
                                matches = false;
                            } else {
                                const op = operands[0];
                                if (op.type == zydis.ZYDIS_OPERAND_TYPE_MEMORY) {
                                    if (op.unnamed_0.mem.disp.has_displacement != @intFromBool(requires_disp)) matches = false;
                                } else {
                                    if (requires_disp) matches = false;
                                }
                            }
                        }

                        if (options.src_disp) |requires_disp| {
                            if (inst.operand_count < 2) {
                                matches = false;
                            } else {
                                const op = operands[1];
                                if (op.type == zydis.ZYDIS_OPERAND_TYPE_MEMORY) {
                                    if (op.unnamed_0.mem.disp.has_displacement != @intFromBool(requires_disp)) matches = false;
                                } else {
                                    if (requires_disp) matches = false;
                                }
                            }
                        }

                        if (matches) {
                            if (skip_remaining == 0) return Address.init(current);
                            skip_remaining -= 1;
                        }
                    }
                }

                if (current + inst.length > self.module.base_address + self.module.sizeOfImage)
                    return MemError.OutOfBounds;

                current += if (inst.length > 0) inst.length else 1;
            }

            return MemError.NoResult;
        }

        pub const InstructionRegisters = struct {
            dst: ?zydis.ZydisRegister = null,
            src: ?zydis.ZydisRegister = null,
        };

        pub fn getRegisters(_: Scanner, addr: usize) !InstructionRegisters {
            var decoder = try makeDecoder();

            var buffer: [32]u8 = undefined;
            _ = Utils.readInstructionBytes(addr, &buffer);

            var inst: zydis.ZydisDecodedInstruction = undefined;
            var operands: [zydis.ZYDIS_MAX_OPERAND_COUNT]zydis.ZydisDecodedOperand = undefined;

            if (zydis.ZydisDecoderDecodeFull(&decoder, &buffer, buffer.len, &inst, &operands) != zydis.ZYAN_STATUS_SUCCESS) {
                return MemError.DecodeFailed;
            }

            var result = InstructionRegisters{};

            if (inst.operand_count >= 1) {
                const dst_op = operands[0];
                switch (dst_op.type) {
                    zydis.ZYDIS_OPERAND_TYPE_REGISTER => result.dst = dst_op.unnamed_0.reg.value,
                    zydis.ZYDIS_OPERAND_TYPE_MEMORY => {
                        if (dst_op.unnamed_0.mem.base != zydis.ZYDIS_REGISTER_NONE)
                            result.dst = dst_op.unnamed_0.mem.base;
                    },
                    else => {},
                }
            }

            if (inst.operand_count >= 2) {
                const src_op = operands[1];
                switch (src_op.type) {
                    zydis.ZYDIS_OPERAND_TYPE_REGISTER => result.src = src_op.unnamed_0.reg.value,
                    zydis.ZYDIS_OPERAND_TYPE_MEMORY => {
                        if (src_op.unnamed_0.mem.base != zydis.ZYDIS_REGISTER_NONE)
                            result.src = src_op.unnamed_0.mem.base;
                    },
                    else => {},
                }
            }

            return result;
        }
    };
};
