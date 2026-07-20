const std = @import("std");

pub fn fnv1_32(data: []const u8) u32 {
    var hash: u32 = 0x811c9dc5;
    for (data) |byte| {
        hash *%= 0x01000193;
        hash ^= @intCast(byte);
    }
    return hash;
}

test "fnv1_32" {
    try std.testing.expectEqual(0xB6FA7167, fnv1_32("hello"));
}
