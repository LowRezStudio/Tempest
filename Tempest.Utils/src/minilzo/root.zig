const std = @import("std");
const assert = std.debug.assert;

const minilzo = @import("minilzo_c");

pub const LZOError = error{
    InitFailed,
    CompressionFailed,
    DecompressionFailed,
    OutputOverrun,
    InputOverrun,
    LookbehindOverrun,
    EOFNotFound,
    InputNotConsumed,
};

var lzo_inititalized: bool = false;

pub fn init() !void {
    if (lzo_inititalized) return;

    if (minilzo.lzo_init() != minilzo.LZO_E_OK) {
        return LZOError.InitFailed;
    }

    lzo_inititalized = true;
    std.log.info("minilzo initialized", .{});
}
