const std = @import("std");

pub const Error = std.Io.Reader.Error || std.mem.Allocator.Error || error{
    UnsupportedEncoding,
    NegativeArrayCount,
};

pub fn TArray(comptime T: type) type {
    return struct {
        data: []T,

        const Self = @This();

        pub fn read() Error!Self {
            return error.NotImplemented;
        }

        pub fn deinit(self: Self, allocator: std.mem.Allocator) void {
            allocator.free(self.data);
        }
    };
}

pub const FString = struct {
    data: []const u8,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!FString {
        const count = try reader.takeInt(i32, .little);
        if (count == 0) return .{ .data = try allocator.alloc(u8, 0) };

        // ANSICHAR
        if (count > 0) {
            const len: usize = @intCast(count);
            const data = try reader.readAlloc(allocator, @intCast(len));
            defer allocator.free(data);

            // strip trailing null
            const str_len = if (len > 0 and data[len - 1] == 0) len - 1 else len;
            return .{ .data = try allocator.dupe(u8, data[0..str_len]) };
        } else {
            // WIDECHAR
            return error.UnsupportedEncoding;
        }
    }
};

pub const FGuid = extern struct {
    a: i32,
    b: i32,
    c: i32,
    d: i32,

    pub fn take(reader: *std.Io.Reader) Error!FGuid {
        return try reader.takeStruct(FGuid, .little);
    }

    pub fn eql(a: FGuid, b: FGuid) bool {
        return a.a == b.a and a.b == b.b and a.c == b.c and a.d == b.d;
    }

    pub fn format(
        self: @This(),
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        const a: u32 = @bitCast(self.a);
        const b: u32 = @bitCast(self.b);
        const c: u32 = @bitCast(self.c);
        const d: u32 = @bitCast(self.d);

        try writer.print("{X:0>8}-{X:0>4}-{X:0>4}-{X:0>4}-{X:0>12}", .{
            a,
            b >> 16,
            b & 0xFFFF,
            c >> 16,
            (@as(u64, c & 0xFFFF) << 32) | @as(u64, d),
        });
    }
};
