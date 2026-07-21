const std = @import("std");
const mem = std.mem;

const FGuid = @import("core.zig").FGuid;
const FName = @import("core.zig").FName;

pub const UGuidCache = extern struct {
    name: FName,
    guid: FGuid,

    pub fn take(r: *std.Io.Reader, allocator: mem.Allocator) !UGuidCache {
        const name = try FName.take(r, allocator, false);
        const guid = try r.takeStruct(FGuid, .little);
        return UGuidCache{ .name = name, .guid = guid };
    }

    pub fn takeArray(r: *std.Io.Reader, allocator: mem.Allocator) ![]UGuidCache {
        const count = try r.takeInt(u32, .little);
        const caches = try allocator.alloc(UGuidCache, count);
        errdefer allocator.free(caches);

        for (caches) |*cache| {
            cache.* = try UGuidCache.take(r, allocator);
        }

        return caches;
    }

    pub fn write(self: UGuidCache, w: *std.Io.Writer) !void {
        try FName.write(self.name, w, false);
        try w.writeStruct(self.guid, .little);
    }

    pub fn writeArray(self: []const UGuidCache, w: *std.Io.Writer) !void {
        try w.writeInt(u32, @intCast(self.len), .little);
        for (self) |cache| {
            try cache.write(w);
        }
    }

    pub fn format(self: UGuidCache, writer: *std.Io.Writer) !void {
        try writer.print(
            \\UGuidCache:
            \\  name: {d}
            \\  guid: {f}
            \\
            \\
        , .{
            self.name,
            self.guid,
        });
    }
};
