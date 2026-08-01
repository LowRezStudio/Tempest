const std = @import("std");
const Io = std.Io;

pub const minilzo = @import("minilzo");

test {
    _ = @import("upk-parser/parser.zig");
    _ = @import("upk-parser/unreal.zig");
    _ = @import("upk-parser/compression.zig");
}
