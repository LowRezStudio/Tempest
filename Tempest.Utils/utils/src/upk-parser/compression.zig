const std = @import("std");
const minilzo = @import("minilzo");
const unreal = @import("unreal.zig");

/// UE ECompressionFlags bit values (from Core/UnFile.h).
pub const compress_none: u32 = 0x00;
pub const compress_zlib: u32 = 0x01;
pub const compress_lzo: u32 = 0x02;
pub const compress_lzx: u32 = 0x04;
pub const compress_bias_memory: u32 = 0x10;
pub const compress_bias_speed: u32 = 0x20;
pub const compress_obscured: u32 = 0x200;

pub const default_codec = compress_lzo;
pub const default_chunk_size: u32 = 0x20000;

pub const Error = std.mem.Allocator.Error || std.Io.Reader.Error || error{
    UnsupportedCompression,
    DecompressionFailed,
    CompressionFailed,
    CorruptData,
    InvalidSize,
    InvalidFormat,
};

fn mapLZOError(err: anyerror) Error {
    return switch (err) {
        error.InvalidSize => error.InvalidSize,
        error.InitFailed, error.DecompressionFailed => error.DecompressionFailed,
        error.CompressionFailed => error.CompressionFailed,
        error.OutputOverrun,
        error.InputOverrun,
        error.LookbehindOverrun,
        error.EOFNotFound,
        error.InputNotConsumed,
        => error.CorruptData,
        error.OutOfMemory => error.OutOfMemory,
        else => error.DecompressionFailed,
    };
}

/// XOR every byte in place with 0x2A (compress_obscured). Symmetric - the same
/// transform both obscures and de-obscures.
pub fn obscure(buf: []u8) void {
    for (buf) |*b| b.* ^= 0x2A;
}

/// Decompress one buffer with the codec selected by `flags`, returning an owned
/// buffer of `uncompressed_size` bytes. Handles the compress_obscured XOR
/// layer automatically.
pub fn decompressMemory(
    allocator: std.mem.Allocator,
    flags: u32,
    compressed: []const u8,
    uncompressed_size: usize,
) Error![]u8 {
    const codec = flags & 0xF;
    return switch (codec) {
        compress_lzo => minilzo.decompressMemory(
            allocator,
            compressed,
            uncompressed_size,
            flags & compress_obscured != 0,
        ) catch |err| return mapLZOError(err),
        compress_zlib => zlibDecompress(
            allocator,
            compressed,
            uncompressed_size,
            flags & compress_obscured != 0,
        ),
        compress_none => blk: {
            // Raw copy - used by uncompressed chunks.
            if (compressed.len != uncompressed_size) return error.CorruptData;
            break :blk try allocator.dupe(u8, compressed);
        },
        else => error.UnsupportedCompression,
    };
}

/// Compress `data` with the codec selected by `flags`, applying the
/// compress_obscured XOR layer on top. Returns an owned
/// buffer of the compressed bytes.
pub fn compressMemory(allocator: std.mem.Allocator, flags: u32, data: []const u8) Error![]u8 {
    const codec = flags & 0xF;
    const out: []u8 = switch (codec) {
        compress_lzo => minilzo.compress(allocator, data) catch |err| return mapLZOError(err),
        compress_zlib => try zlibCompress(allocator, data),
        else => return error.UnsupportedCompression,
    };
    errdefer allocator.free(out);

    if (flags & compress_obscured != 0) obscure(out);
    return out;
}

fn zlibDecompress(
    allocator: std.mem.Allocator,
    compressed: []const u8,
    uncompressed_size: usize,
    obscured: bool,
) Error![]u8 {
    var data = compressed;
    var scratch: ?[]u8 = null;
    defer if (scratch) |s| allocator.free(s);

    if (obscured) {
        scratch = try allocator.dupe(u8, compressed);
        obscure(scratch.?);
        data = scratch.?;
    }

    var in: std.Io.Reader = .fixed(data);
    var out: std.Io.Writer.Allocating = .init(allocator);
    defer out.deinit();

    var decompress: std.compress.flate.Decompress = .init(&in, .zlib, &.{});
    const written = decompress.reader.streamRemaining(&out.writer) catch |err| switch (err) {
        error.WriteFailed => return error.OutOfMemory,
        error.ReadFailed => {
            std.log.debug("zlib decode failed: {s}", .{@errorName(decompress.err orelse error.DecompressionFailed)});
            return error.DecompressionFailed;
        },
    };
    if (written != uncompressed_size) return error.CorruptData;

    return out.toOwnedSlice();
}

fn zlibCompress(allocator: std.mem.Allocator, data: []const u8) Error![]u8 {
    var out: std.Io.Writer.Allocating = try .initCapacity(allocator, data.len / 2 + 64);
    defer out.deinit();

    var window: [std.compress.flate.max_window_len]u8 = undefined;
    var compress = std.compress.flate.Compress.init(&out.writer, &window, .zlib, .default) catch
        return error.CompressionFailed;
    compress.writer.writeAll(data) catch return error.CompressionFailed;
    compress.finish() catch return error.CompressionFailed;

    return out.toOwnedSlice();
}

/// One entry of a `SerializeCompressed` stream's chunk table.
pub const CompressedChunkInfo = struct {
    compressed_size: i32,
    uncompressed_size: i32,
};

/// Decompress a `SerializeCompressed` stream into one owned buffer.
/// A chunk with a negative CompressedSize is stored raw (not compressed).
pub fn decompressStream(allocator: std.mem.Allocator, data: []const u8, flags: u32) Error![]u8 {
    var r: std.Io.Reader = .fixed(data);

    const tag = try r.takeInt(u32, .little);
    if (tag == unreal.package_file_tag_swapped) return error.InvalidFormat;
    if (tag != unreal.package_file_tag) return error.CorruptData;

    const chunk_size = try r.takeInt(u32, .little);
    if (chunk_size == 0) return error.CorruptData;
    const total_compressed_size = try r.takeInt(u32, .little);
    const total_uncompressed_size = try r.takeInt(u32, .little);

    const chunk_count = (total_uncompressed_size + chunk_size - 1) / chunk_size;
    if (chunk_count == 0 or chunk_count > 1 << 20) return error.CorruptData;

    const chunks = try allocator.alloc(CompressedChunkInfo, chunk_count);
    defer allocator.free(chunks);
    var sum_compressed: u64 = 0;
    for (chunks) |*chunk| {
        chunk.compressed_size = try r.takeInt(i32, .little);
        chunk.uncompressed_size = try r.takeInt(i32, .little);
        sum_compressed += @abs(@as(i64, chunk.compressed_size));
    }
    if (sum_compressed != @as(u64, total_compressed_size)) return error.CorruptData;

    var result = try allocator.alloc(u8, total_uncompressed_size);
    errdefer allocator.free(result);

    var pos: usize = 0;
    for (chunks) |chunk| {
        const is_raw = chunk.compressed_size < 0;
        const cs: usize = @intCast(if (is_raw) -@as(i64, chunk.compressed_size) else chunk.compressed_size);
        const us: usize = @intCast(chunk.uncompressed_size);
        if (pos + us > total_uncompressed_size) return error.CorruptData;

        const payload = try r.readAlloc(allocator, cs);
        defer allocator.free(payload);

        if (is_raw) {
            if (cs != us) return error.CorruptData;
            @memcpy(result[pos .. pos + us], payload);
        } else {
            const dec = try decompressMemory(allocator, flags, payload, us);
            defer allocator.free(dec);
            @memcpy(result[pos .. pos + us], dec);
        }
        pos += us;
    }

    if (pos != total_uncompressed_size) return error.CorruptData;
    return result;
}

test "obscure is symmetric" {
    var buf = [_]u8{ 0x00, 0x2A, 0xFF, 0x10 };
    const copy = buf;
    obscure(&buf);
    obscure(&buf);
    try std.testing.expectEqualSlices(u8, &copy, &buf);
}

test "lzo round trip" {
    const a = std.testing.allocator;
    var data: [2048]u8 = undefined;
    for (&data, 0..) |*b, i| b.* = @truncate(i * 7 + 3);

    const compressed = try compressMemory(a, compress_lzo, &data);
    defer a.free(compressed);
    const dec = try decompressMemory(a, compress_lzo, compressed, data.len);
    defer a.free(dec);
    try std.testing.expectEqualSlices(u8, &data, dec);
}

test "lzo obscured round trip" {
    const a = std.testing.allocator;
    var data: [512]u8 = undefined;
    for (&data, 0..) |*b, i| b.* = @truncate(i + 1);

    const flags = compress_lzo | compress_obscured;
    const compressed = try compressMemory(a, flags, &data);
    defer a.free(compressed);
    const dec = try decompressMemory(a, flags, compressed, data.len);
    defer a.free(dec);
    try std.testing.expectEqualSlices(u8, &data, dec);
}

test "zlib round trip" {
    const a = std.testing.allocator;
    var data: [1024]u8 = undefined;
    for (&data, 0..) |*b, i| b.* = @truncate(i * 3);

    const compressed = try compressMemory(a, compress_zlib, &data);
    defer a.free(compressed);
    const dec = try decompressMemory(a, compress_zlib, compressed, data.len);
    defer a.free(dec);
    try std.testing.expectEqualSlices(u8, &data, dec);
}

pub fn compressStream(a: std.mem.Allocator, flags: u32, data: []const u8) ![]u8 {
    const sub: usize = default_chunk_size;
    const n = (data.len + sub - 1) / sub;
    const infos = try a.alloc(CompressedChunkInfo, n);
    defer a.free(infos);
    const payloads = try a.alloc([]u8, n);
    defer a.free(payloads);
    errdefer for (payloads) |p| if (p.len > 0) a.free(p);

    var total_compressed: usize = 0;
    var off: usize = 0;
    for (infos, 0..) |*info, i| {
        const us = @min(sub, data.len - off);
        const compressed = try compressMemory(a, flags, data[off .. off + us]);
        payloads[i] = compressed;
        info.compressed_size = @intCast(compressed.len);
        info.uncompressed_size = @intCast(us);
        total_compressed += compressed.len;
        off += us;
    }

    var out: std.Io.Writer.Allocating = .init(a);
    defer out.deinit();
    const w = &out.writer;
    try w.writeInt(u32, unreal.package_file_tag, .little);
    try w.writeInt(u32, default_chunk_size, .little);
    try w.writeInt(u32, @intCast(total_compressed), .little);
    try w.writeInt(u32, @intCast(data.len), .little);
    for (infos) |info| {
        try w.writeInt(i32, info.compressed_size, .little);
        try w.writeInt(i32, info.uncompressed_size, .little);
    }
    for (payloads) |p| {
        try w.writeAll(p);
        a.free(p);
    }
    return out.toOwnedSlice();
}

test "stream round trip" {
    const a = std.testing.allocator;
    var data: [70000]u8 = undefined; // spans multiple 0x20000 chunks if chunked
    for (&data, 0..) |*b, i| b.* = @truncate(i * 11);

    const stream = try compressStream(a, compress_lzo, &data);
    defer a.free(stream);

    const dec = try decompressStream(a, stream, compress_lzo);
    defer a.free(dec);
    try std.testing.expectEqualSlices(u8, &data, dec);
}

test "stream round trip obscured" {
    const a = std.testing.allocator;
    var data: [70000]u8 = undefined;
    for (&data, 0..) |*b, i| b.* = @truncate(i * 13);

    const flags = compress_lzo | compress_obscured;
    const stream = try compressStream(a, flags, &data);
    defer a.free(stream);

    const dec = try decompressStream(a, stream, flags);
    defer a.free(dec);
    try std.testing.expectEqualSlices(u8, &data, dec);
}

pub fn decompressPackage(
    allocator: std.mem.Allocator,
    file: []const u8,
    chunks: []const unreal.FCompressedChunk,
    flags: u32,
) Error![]u8 {
    if (chunks.len == 0) return error.CorruptData;

    const first = chunks[0];
    const last = chunks[chunks.len - 1];
    const total: usize = @intCast(@as(i64, last.uncompressed_offset) + last.uncompressed_size);
    if (total == 0 or total > 0x4000_0000) return error.CorruptData; // sanity cap 1 GiB

    var result = try allocator.alloc(u8, total);
    errdefer allocator.free(result);
    @memset(result, 0);

    // Bytes before the first compressed chunk are the plain header section.
    const prefix = @min(
        @as(usize, @intCast(first.compressed_offset)),
        @as(usize, @intCast(first.uncompressed_offset)),
    );
    if (prefix > file.len or prefix > total) return error.CorruptData;
    @memcpy(result[0..prefix], file[0..prefix]);

    for (chunks) |chunk| {
        const is_raw = chunk.compressed_size < 0;
        const cs: usize = @intCast(if (is_raw) -@as(i64, chunk.compressed_size) else chunk.compressed_size);
        const co: usize = @intCast(chunk.compressed_offset);
        const uo: usize = @intCast(chunk.uncompressed_offset);
        const us: usize = @intCast(chunk.uncompressed_size);

        if (co + cs > file.len or uo + us > total) return error.CorruptData;
        const payload = file[co .. co + cs];

        if (is_raw) {
            if (cs != us) return error.CorruptData;
            @memcpy(result[uo .. uo + us], payload);
        } else {
            const dec = try decompressStream(allocator, payload, flags);
            defer allocator.free(dec);
            if (dec.len != us) return error.CorruptData;
            @memcpy(result[uo .. uo + us], dec);
        }
    }

    return result;
}
