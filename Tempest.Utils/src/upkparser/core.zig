const std = @import("std");
const mem = std.mem;

pub const FGuid = extern struct {
    a: u32 = 0,
    b: u32 = 0,
    c: u32 = 0,
    d: u32 = 0,

    pub fn take(reader: *std.Io.Reader) !FGuid {
        return try reader.takeStruct(FGuid, .little);
    }

    pub fn takeArray(reader: *std.Io.Reader, allocator: mem.Allocator) ![]FGuid {
        const count = try reader.takeInt(u32, .little);
        const guids = try allocator.alloc(FGuid, count);
        errdefer allocator.free(guids);

        for (guids) |*guid| {
            guid.* = try FGuid.take(reader);
        }
        return guids;
    }

    pub fn write(self: FGuid, writer: *std.io.Writer) !void {
        try writer.writeStruct(self, .little);
    }

    pub fn writeArray(self: []const FGuid, writer: *std.io.Writer, count: u32) !void {
        try writer.writeInt(u32, count, .little);
        for (self) |guid| {
            try guid.write(writer);
        }
    }

    pub fn format(self: FGuid, writer: *std.Io.Writer) !void {
        try writer.print("{X}-{X}-{X}-{X}", .{
            self.a,
            self.b,
            self.c,
            self.d,
        });
    }
};

pub const FNameEntry = extern struct {
    name: FName,
    flags: u64,

    pub fn take(reader: *std.Io.Reader, allocator: mem.Allocator) !FNameEntry {
        const name = try FName.take(reader, allocator);
        const flags = try reader.takeInt(u64, .little);
        return FNameEntry{ .name = name, .flags = flags };
    }

    pub fn write(self: FNameEntry, w: *std.io.Writer) !void {
        try self.name.write(w);
        try w.writeInt(u64, self.flags, .little);
    }
};

pub const FName = extern struct {
    len: u32,
    data: [*:0]u8,

    pub fn take(reader: *std.Io.Reader, allocator: mem.Allocator) !FName {
        const len: u32 = try reader.takeInt(u32, .little);
        const data = try reader.readAlloc(allocator, @intCast(len));
        errdefer allocator.free(data);

        return FName{ .len = len, .data = @ptrCast(data) };
    }

    pub fn takeArray(reader: *std.Io.Reader, allocator: mem.Allocator) ![]FName {
        const count: u32 = try reader.takeInt(u32, .little);
        const names = try allocator.alloc(FName, count);
        errdefer allocator.free(names);

        for (names) |*name| {
            name.* = try FName.take(reader, allocator);
        }

        return names;
    }

    pub fn write(self: FName, w: *std.io.Writer) !void {
        try w.writeInt(u32, self.len, .little);
        try w.writeAll(self.data[0..self.len]);
    }

    pub fn writeArray(self: []const FName, w: *std.io.Writer) !void {
        try w.writeInt(u32, @intCast(self.len), .little);
        for (self) |name| {
            try name.write(w);
        }
    }

    pub fn toString(self: FName) []const u8 {
        return self.data[0..self.len];
    }

    pub fn format(self: FName, writer: *std.io.Writer) !void {
        try writer.print("{s}", .{self.toString()});
    }
};
