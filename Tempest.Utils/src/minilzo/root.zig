const std = @import("std");
const assert = std.debug.assert;

const minilzo = @import("minilzo_c");

pub const LZOError = error{
    InitFailed,
    InvalidSize,
    OutputOverrun,
    InputOverrun,
    LookbehindOverrun,
    EOFNotFound,
    InputNotConsumed,
    DecompressionFailed,
};

var lzo_inititalized: bool = false;

pub fn init() LZOError!void {
    if (lzo_inititalized) return;

    if (minilzo.lzo_init() != minilzo.LZO_E_OK) {
        return LZOError.InitFailed;
    }

    lzo_inititalized = true;
    std.log.info("minilzo initialized", .{});
}

fn decryptBuffer(allocator: std.mem.Allocator, buffer: []const u8) ![]u8 {
    const decrypted = try allocator.alloc(u8, buffer.len);
    const key: u8 = 0x2A;

    for (buffer, 0..) |byte, i| {
        decrypted[i] = byte ^ key;
    }

    return decrypted;
}

pub fn decompressMemory(
    allocator: std.mem.Allocator,
    compressed_buffer: []const u8,
    uncompressed_size: usize,
    encrypted: bool,
) ![]u8 {
    try init();

    const buffer_to_decompress = if (encrypted)
        try decryptBuffer(allocator, compressed_buffer)
    else
        compressed_buffer;
    defer if (encrypted) allocator.free(buffer_to_decompress);

    const uncompressed_buffer = try allocator.alloc(u8, uncompressed_size);
    errdefer allocator.free(uncompressed_buffer);

    var final_uncompressed_size: minilzo.lzo_uint = @intCast(uncompressed_size);
    const result = minilzo.lzo1x_decompress_safe(
        buffer_to_decompress.ptr,
        @intCast(buffer_to_decompress.len),
        uncompressed_buffer.ptr,
        &final_uncompressed_size,
        null,
    );

    switch (result) {
        minilzo.LZO_E_OK => {
            if (final_uncompressed_size != uncompressed_size) {
                allocator.free(uncompressed_buffer);
                std.log.err("Size mismatch: expected {d}, got {d}", .{ uncompressed_size, final_uncompressed_size });
                return LZOError.InvalidSize;
            }
            return uncompressed_buffer;
        },
        minilzo.LZO_E_OUTPUT_OVERRUN => return LZOError.OutputOverrun,
        minilzo.LZO_E_INPUT_OVERRUN => return LZOError.InputOverrun,
        minilzo.LZO_E_LOOKBEHIND_OVERRUN => return LZOError.LookbehindOverrun,
        minilzo.LZO_E_EOF_NOT_FOUND => return LZOError.EOFNotFound,
        minilzo.LZO_E_INPUT_NOT_CONSUMED => return LZOError.InputNotConsumed,
        else => return LZOError.DecompressionFailed,
    }
}
