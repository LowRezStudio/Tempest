const std = @import("std");
const windows = std.os.windows;

const mcts = @import("mcts.zig");

pub const CPackageStringStream = struct {
    package: *mcts.CPackage,
    available: windows.WORD,
    read: windows.WORD,
    underflowed: bool,

    pub fn take(self: *CPackageStringStream) u64 {
        if (self.read >= self.available) {
            self.underflowed = true;
            return 0;
        }

        const pkg = self.package;
        const place: u64 = pkg.place;
        var c: u8 = 0;
        var result: u64 = undefined;

        if (place + 1 >= pkg.used or pkg.cur_place + 1 > 0x7fd) {
            _ = pkg.read(@as([*]u8, @ptrCast(&c))[0..1], 1);
            result = c;
        } else {
            result = pkg.current.?.net.data[pkg.cur_place];
            pkg.cur_place += 1;
            pkg.place += 1;
        }

        self.read += 1;
        return result;
    }
};

pub const HirezRapidJson = struct {
    pub const UTF8: type = []const u8;

    pub const UTF16Wchar: type = []const u16;
    pub const UTF16Unsigned: type = u16;
    pub const UTF16ShortUnsigned: type = u16;

    pub const UTF32Wchar: type = []const u16;
    pub const UTF32Unsigned: type = u32;

    // zig fmt: off
    const utf8_range_table = [256]u8{
        0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
        0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
        0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
        0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
        0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
        0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
        0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
        0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
        0x10,0x10,0x10,0x10,0x10,0x10,0x10,0x10,0x10,0x10,0x10,0x10,0x10,0x10,0x10,0x10,
        0x40,0x40,0x40,0x40,0x40,0x40,0x40,0x40,0x40,0x40,0x40,0x40,0x40,0x40,0x40,0x40,
        0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,
        0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,0x20,
        8,8,2,2,2,2,2,2,2,2,2,2,2,2,2,2,
        2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,
        10,3,3,3,3,3,3,3,3,3,3,3,3,4,3,3,
        11,6,6,6,5,8,8,8,8,8,8,8,8,8,8,8,
    };
    // zig fmt: on

    pub fn getRange(c: u8) u8 {
        return utf8_range_table[c];
    }

    inline fn packageReadByte(is: *CPackageStringStream) struct { byte: u8, ok: bool } {
        if (is.read >= is.available) {
            is.underflowed = true;
            return .{ .byte = 0, .ok = false };
        }

        const pkg = is.package;
        const place: u64 = pkg.place;
        var b: u8 = undefined;

        if (place + 1 >= pkg.used or pkg.cur_place + 1 > 0x7fd) {
            var buf: [1]u8 = undefined;
            _ = pkg.read(&buf, 1);
            b = buf[0];
        } else {
            b = pkg.current.?.net.data[pkg.cur_place];
            pkg.cur_place += 1;
            pkg.place += 1;
        }

        is.read += 1;
        return .{ .byte = b, .ok = true };
    }

    inline fn copy(is: *CPackageStringStream, codepoint: *u32) u8 {
        const c: u8 = @truncate(is.take());
        codepoint.* = (codepoint.* << 6) | (c & 0x3f);
        return c;
    }

    inline fn trans(c: u8, mask: u8, result: *bool) void {
        result.* = result.* and (getRange(c) & mask) != 0;
    }

    inline fn tail(is: *CPackageStringStream, codepoint: *u32, result: *bool) void {
        trans(copy(is, codepoint), 0x70, result);
    }

    // Hirez implementation of RapidJson::Decode
    pub fn decode(is: *CPackageStringStream, codepoint: *u32) bool {
        if (is.read >= is.available) {
            is.underflowed = true;
            codepoint.* = 0;
            return true;
        }

        const pkg = is.package;
        const place: u64 = pkg.place;
        var b0: u8 = undefined;

        if (place + 1 >= pkg.used or pkg.cur_place + 1 > 0x7fd) {
            var buf: [1]u8 = undefined;
            _ = pkg.read(&buf, 1);
            b0 = buf[0];
        } else {
            b0 = pkg.current.?.net.data[pkg.cur_place];
            pkg.cur_place += 1;
            pkg.place += 1;
        }

        is.read += 1;

        if (@as(i8, @bitCast(b0)) >= 0) {
            codepoint.* = b0;
            return true;
        }

        const range = getRange(b0);

        if (range > 0x1f) {
            codepoint.* = 0;
            return false;
        }

        codepoint.* = @as(u32, b0) & (@as(u32, 0xff) >> @intCast(range));

        if (range > 0xb) return false;

        var result: bool = true;

        switch (range) {
            2 => {
                tail(is, codepoint, &result);
            },
            3 => {
                tail(is, codepoint, &result);
                tail(is, codepoint, &result);
            },
            4 => {
                trans(copy(is, codepoint), 0x50, &result);
                tail(is, codepoint, &result);
            },
            5 => {
                const r1 = packageReadByte(is);
                codepoint.* = (codepoint.* << 6) | (r1.byte & 0x3f);
                const r1_shift: u32 = getRange(r1.byte) >> 4;

                const r2 = packageReadByte(is);
                codepoint.* = (codepoint.* << 6) | (r2.byte & 0x3f);
                const valid_r2: u32 = getRange(r2.byte) & 0x70;

                const r3 = packageReadByte(is);
                codepoint.* = (codepoint.* << 6) | (r3.byte & 0x3f);
                const valid_r3: u32 = getRange(r3.byte) & 0x70;

                return (valid_r3 & r1_shift & 1 & (valid_r2 >> 4)) != 0;
            },
            6, 0xb => {
                const c1 = copy(is, codepoint);
                const r1 = getRange(c1);
                const c2 = copy(is, codepoint);
                const cond6_1: u8 = if (range == 0xb) r1 & 0x60 else r1 & 0x70;
                const valid_c2 = getRange(c2) & 0x70;
                // label_75a1c8:
                const c3 = copy(is, codepoint);
                const valid_c3: u8 = @intFromBool((getRange(c3) & 0x70) != 0);
                return (valid_c3 & @as(u8, @intFromBool(cond6_1 != 0)) & @as(u8, @intFromBool(valid_c2 != 0))) != 0;
            },
            0xa => {
                const c1 = copy(is, codepoint);
                const r1 = getRange(c1);
                const c2 = copy(is, codepoint);
                const valid_c2 = getRange(c2) & 0x70;
                return ((r1 >> 5) & (valid_c2 >> 4)) != 0;
            },
            else => return false,
        }

        return result;
    }
};
