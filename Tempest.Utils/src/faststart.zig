//! faststart: startup-time fixes for the Paladins 8.1 Win64 client.
//!
//! The CLI injects this DLL into the client right after it starts. Every patch is tied to
//! one build of the game: the image has to carry the expected link timestamp and size, and
//! each site its expected bytes, or the patch is left out. Nothing here changes what the
//! game computes, only how long it takes. See docs/faststart.md.

const std = @import("std");
const windows = std.os.windows;

const detourz = @import("detourz");

const BOOL = windows.BOOL;
const HANDLE = windows.HANDLE;

const FILETIME = extern struct { low: u32, high: u32 };

const BY_HANDLE_FILE_INFORMATION = extern struct {
    attributes: u32,
    creation_time: FILETIME,
    last_access_time: FILETIME,
    last_write_time: FILETIME,
    volume_serial: u32,
    size_high: u32,
    size_low: u32,
    links: u32,
    index_high: u32,
    index_low: u32,
};

const WIN32_FILE_ATTRIBUTE_DATA = extern struct {
    attributes: u32,
    creation_time: FILETIME,
    last_access_time: FILETIME,
    last_write_time: FILETIME,
    size_high: u32,
    size_low: u32,
};

extern "kernel32" fn GetModuleHandleW(name: ?[*:0]const u16) callconv(.winapi) ?*anyopaque;
extern "kernel32" fn GetCurrentProcess() callconv(.winapi) HANDLE;
extern "kernel32" fn GetCurrentProcessId() callconv(.winapi) u32;
extern "kernel32" fn GetCurrentThread() callconv(.winapi) HANDLE;
extern "kernel32" fn GetCurrentThreadId() callconv(.winapi) u32;
extern "kernel32" fn GetTickCount() callconv(.winapi) u32;
extern "kernel32" fn GetLastError() callconv(.winapi) u32;
extern "kernel32" fn SetLastError(code: u32) callconv(.winapi) void;
extern "kernel32" fn Sleep(ms: u32) callconv(.winapi) void;
extern "kernel32" fn QueryPerformanceCounter(counter: *i64) callconv(.winapi) BOOL;
extern "kernel32" fn QueryPerformanceFrequency(frequency: *i64) callconv(.winapi) BOOL;
extern "kernel32" fn SetFilePointerEx(file: HANDLE, distance: i64, new: ?*i64, method: u32) callconv(.winapi) BOOL;
extern "kernel32" fn ReadFile(file: HANDLE, buffer: [*]u8, len: u32, read: *u32, overlapped: ?*anyopaque) callconv(.winapi) BOOL;
extern "kernel32" fn EnterCriticalSection(section: *anyopaque) callconv(.winapi) void;
extern "kernel32" fn LeaveCriticalSection(section: *anyopaque) callconv(.winapi) void;
extern "kernel32" fn SwitchToThread() callconv(.winapi) BOOL;
extern "kernel32" fn LoadLibraryA(name: [*:0]const u8) callconv(.winapi) ?*anyopaque;
extern "kernel32" fn GetProcAddress(module: *anyopaque, name: [*:0]const u8) callconv(.winapi) ?*anyopaque;
extern "kernel32" fn VirtualProtect(address: *anyopaque, size: usize, protect: u32, old: *u32) callconv(.winapi) BOOL;
extern "kernel32" fn VirtualAlloc(address: ?*anyopaque, size: usize, kind: u32, protect: u32) callconv(.winapi) ?[*]u8;
extern "kernel32" fn VirtualFree(address: *anyopaque, size: usize, kind: u32) callconv(.winapi) BOOL;
extern "kernel32" fn FlushInstructionCache(process: HANDLE, address: ?*const anyopaque, size: usize) callconv(.winapi) BOOL;
extern "kernel32" fn CreateThread(attributes: ?*anyopaque, stack: usize, start: *const fn (?*anyopaque) callconv(.winapi) u32, param: ?*anyopaque, flags: u32, id: ?*u32) callconv(.winapi) ?HANDLE;
extern "kernel32" fn CloseHandle(handle: HANDLE) callconv(.winapi) BOOL;
extern "kernel32" fn CreateFileW(name: [*:0]const u16, access: u32, share: u32, security: ?*anyopaque, disposition: u32, flags: u32, template: ?HANDLE) callconv(.winapi) HANDLE;
extern "kernel32" fn WriteFile(file: HANDLE, buffer: [*]const u8, len: u32, written: ?*u32, overlapped: ?*anyopaque) callconv(.winapi) BOOL;
extern "kernel32" fn GetTempPathW(len: u32, buffer: [*]u16) callconv(.winapi) u32;
extern "kernel32" fn GetEnvironmentVariableA(name: [*:0]const u8, buffer: [*]u8, size: u32) callconv(.winapi) u32;
extern "kernel32" fn GetFileInformationByHandle(file: HANDLE, info: *BY_HANDLE_FILE_INFORMATION) callconv(.winapi) BOOL;
extern "kernel32" fn GetFileAttributesExW(name: [*:0]const u16, level: u32, info: *WIN32_FILE_ATTRIBUTE_DATA) callconv(.winapi) BOOL;
// RtlGenRandom
extern "advapi32" fn SystemFunction036(buffer: [*]u8, len: u32) callconv(.winapi) u8;

const invalid_handle: HANDLE = @ptrFromInt(std.math.maxInt(usize));

// Paladins 8.1 (1.0.6890.0) Win64 client.
const build_timestamp: u32 = 0x679bfe80;
const build_image_size: u32 = 0x4c0f000;

const rva = struct {
    const rand_poll = 0x1f6dc80;
    const rand_add = 0x1f22b90;
    const file_size = 0x12d7c0;
    const find_package = 0x1aae80;
    const read_compressed = 0x148ac0;
    const uncompress = 0x1c23c0;
    const io_stats_enabled = 0x3876f2c;
    const platform_read = 0x1804b0;
    const crc32 = 0x1bff30;
    const crc32_table = 0x388cd40;
    const voice_init_sleep = 0xfc891;
    const thread_name_sleep = 0x1fbf6f;
    const pool_worker_sleep = 0x1f8982;
};

var base: usize = 0;

// ---------------------------------------------------------------------------
// Log: %TEMP%\tempest-faststart.log, rewritten by each game start.

var log_file: ?HANDLE = null;
var log_mutex: std.Thread.Mutex = .{};

fn openLog() void {
    var path: [300]u16 = undefined;
    const n = GetTempPathW(260, &path);
    if (n == 0 or n > 260) return;
    const name = std.unicode.utf8ToUtf16LeStringLiteral("tempest-faststart.log");
    @memcpy(path[n .. n + name.len], name);
    path[n + name.len] = 0;
    // GENERIC_WRITE, FILE_SHARE_READ, CREATE_ALWAYS
    const file = CreateFileW(@ptrCast(&path), 0x40000000, 1, null, 2, 0x80, null);
    if (file != invalid_handle) log_file = file;
}

fn log(comptime fmt: []const u8, args: anytype) void {
    const file = log_file orelse return;
    var buffer: [512]u8 = undefined;
    const text = std.fmt.bufPrint(&buffer, "[{d:>8}] " ++ fmt ++ "\r\n", .{GetTickCount()} ++ args) catch return;
    log_mutex.lock();
    defer log_mutex.unlock();
    _ = WriteFile(file, text.ptr, @intCast(text.len), null, null);
}

// ---------------------------------------------------------------------------
// Sites

fn at(comptime T: type, offset: usize) T {
    return @ptrFromInt(base + offset);
}

fn matches(offset: usize, expected: []const u8) bool {
    const code: [*]const u8 = @ptrFromInt(base + offset);
    return std.mem.eql(u8, code[0..expected.len], expected);
}

fn isExpectedBuild() bool {
    const image: [*]const u8 = @ptrFromInt(base);
    if (image[0] != 'M' or image[1] != 'Z') return false;
    const nt = std.mem.readInt(u32, image[0x3c..0x40], .little);
    if (nt > 0x1000) return false;
    const timestamp = std.mem.readInt(u32, image[nt + 8 ..][0..4], .little);
    // SizeOfImage of the PE32+ optional header
    const image_size = std.mem.readInt(u32, image[nt + 24 + 56 ..][0..4], .little);
    return timestamp == build_timestamp and image_size == build_image_size;
}

/// Replaces `expected` at `offset` with `patch`, when the bytes there are the expected ones.
fn patchBytes(comptime name: []const u8, offset: usize, expected: []const u8, patch: []const u8) void {
    std.debug.assert(expected.len == patch.len);
    if (!matches(offset, expected)) {
        log("{s}: unexpected bytes, left alone", .{name});
        return;
    }
    const code: [*]u8 = @ptrFromInt(base + offset);
    var old: u32 = 0;
    // PAGE_EXECUTE_READWRITE
    if (VirtualProtect(code, patch.len, 0x40, &old) == 0) {
        log("{s}: cannot unprotect the code page", .{name});
        return;
    }
    @memcpy(code[0..patch.len], patch);
    _ = VirtualProtect(code, patch.len, old, &old);
    _ = FlushInstructionCache(GetCurrentProcess(), code, patch.len);
    log("{s}: patched", .{name});
}

var disabled_buffer: [256]u8 = undefined;
var disabled: []const u8 = "";

/// `TEMPEST_FASTSTART_DISABLE` is a comma separated list of patch names (or `all`).
fn enabled(name: []const u8) bool {
    var it = std.mem.tokenizeAny(u8, disabled, ", ;");
    while (it.next()) |entry| {
        if (std.ascii.eqlIgnoreCase(entry, name) or std.ascii.eqlIgnoreCase(entry, "all")) return false;
    }
    return true;
}

// ---------------------------------------------------------------------------
// rand: OpenSSL's RAND_poll walks every heap, module and thread of the process through
// toolhelp snapshots to collect entropy, for up to a second per list. The system random
// number generator gives better seed material at once.

const RandPoll = *const fn () callconv(.c) c_int;
const RandAdd = *const fn (buffer: *const anyopaque, len: c_int, entropy: f64) callconv(.c) void;

var original_rand_poll: RandPoll = undefined;
var rand_add: RandAdd = undefined;
var rand_polls = std.atomic.Value(u32).init(0);

fn hookRandPoll() callconv(.c) c_int {
    var seed: [64]u8 = undefined;
    if (SystemFunction036(&seed, seed.len) == 0) return original_rand_poll();
    rand_add(&seed, seed.len, 64.0);
    std.crypto.secureZero(u8, &seed);

    // What the original adds last, credited as it credits them.
    var counter: i64 = 0;
    _ = QueryPerformanceCounter(&counter);
    rand_add(&counter, @sizeOf(i64), 0.0);
    var pid = GetCurrentProcessId();
    rand_add(&pid, @sizeOf(u32), 1.0);

    if (rand_polls.fetchAdd(1, .monotonic) == 0) log("rand: seeded from the system generator", .{});
    return 1;
}

// ---------------------------------------------------------------------------
// file_size: the engine opens a file, asks its size and closes it. One attribute query
// answers the same without an open (which is also what antivirus filters scan). Only
// cooked content goes this way; anything else keeps the original path.

const FileSize = *const fn (this: ?*anyopaque, name: ?[*:0]const u16) callconv(.c) i64;

var original_file_size: FileSize = undefined;

/// Cooked content, or a bare name: the package lookup asks for the size of "<package>_SF"
/// in the working directory tens of thousands of times while the item tables load.
fn isFastPath(name: [*:0]const u16) bool {
    const needle = "\\cookedpc";
    var matched: usize = 0;
    var bare = true;
    var i: usize = 0;
    while (name[i] != 0) : (i += 1) {
        var c = name[i];
        if (c == '/') c = '\\';
        if (c == '\\' or c == ':') bare = false;
        if (c >= 'A' and c <= 'Z') c += 'a' - 'A';
        if (c == needle[matched]) {
            matched += 1;
            if (matched == needle.len) return true;
        } else {
            matched = if (c == needle[0]) 1 else 0;
        }
    }
    return bare and i != 0;
}

/// The same few thousand paths are asked for again and again while the tables load, so an
/// answer is kept for a moment. Short enough that a file the game writes itself (a shader
/// cache) is seen with its new size by anything that asks after the save.
const size_memo_ms = 250;

const SizeMemo = struct { hash: u64 = 0, tick: u32 = 0, size: i64 = 0 };

var size_memo: [1024]SizeMemo = @splat(.{});
var size_memo_mutex: std.Thread.Mutex = .{};
var size_memo_hits = std.atomic.Value(u32).init(0);
var size_memo_misses = std.atomic.Value(u32).init(0);

fn queryFileSize(this: ?*anyopaque, path: [*:0]const u16) i64 {
    var info: WIN32_FILE_ATTRIBUTE_DATA = undefined;
    if (GetFileAttributesExW(path, 0, &info) == 0) {
        const err = GetLastError();
        // ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND
        if (err == 2 or err == 3) return -1;
        return original_file_size(this, path);
    }
    // FILE_ATTRIBUTE_DIRECTORY: the original's open fails on one.
    if (info.attributes & 0x10 != 0) return -1;
    return info.size_low;
}

fn hookFileSize(this: ?*anyopaque, name: ?[*:0]const u16) callconv(.c) i64 {
    const path = name orelse return original_file_size(this, name);
    if (!isFastPath(path)) return original_file_size(this, name);

    var hash: u64 = 0xcbf29ce484222325;
    var i: usize = 0;
    while (path[i] != 0) : (i += 1) hash = (hash ^ path[i]) *% 0x100000001b3;
    hash ^= hash >> 31;
    if (hash == 0) hash = 1;
    const slot = &size_memo[@as(usize, @truncate(hash)) & (size_memo.len - 1)];
    const now = GetTickCount();

    size_memo_mutex.lock();
    if (slot.hash == hash and now -% slot.tick < size_memo_ms) {
        const size = slot.size;
        size_memo_mutex.unlock();
        _ = size_memo_hits.fetchAdd(1, .monotonic);
        return size;
    }
    size_memo_mutex.unlock();

    const size = queryFileSize(this, path);
    _ = size_memo_misses.fetchAdd(1, .monotonic);
    size_memo_mutex.lock();
    slot.* = .{ .hash = hash, .tick = now, .size = size };
    size_memo_mutex.unlock();
    return size;
}

// ---------------------------------------------------------------------------
// packages: the item, character and device tables resolve the package of every asset
// they name, and tens of thousands of those names are the same few that no package file
// carries (an empty name plus the seek-free suffix, mostly). Each miss walks three file
// maps with half a dozen allocations and a file query. A miss is remembered for a moment.
//
// Only a miss, for the same name, language and GUID, and only for two seconds: a package
// that appears later is found as before.

const FindPackage = *const fn (this: ?*anyopaque, name: ?[*:0]const u16, guid: ?*const anyopaque, out: ?*anyopaque, language: ?[*:0]const u16) callconv(.c) i32;

var original_find_package: FindPackage = undefined;
const package_miss_ms = 2000;
/// High 40 bits: the name hash. Low 24 bits: when it missed, in 16 ms steps.
var package_misses: [256]std.atomic.Value(u64) = @splat(.init(0));
var package_miss_hits = std.atomic.Value(u32).init(0);

fn hashName(seed: u64, name: ?[*:0]const u16) u64 {
    var hash = seed;
    const text = name orelse return hash *% 0x100000001b3;
    var i: usize = 0;
    while (text[i] != 0) : (i += 1) {
        var c = text[i];
        if (c >= 'A' and c <= 'Z') c += 'a' - 'A';
        hash = (hash ^ c) *% 0x100000001b3;
    }
    return hash;
}

fn hookFindPackage(this: ?*anyopaque, name: ?[*:0]const u16, guid: ?*const anyopaque, out: ?*anyopaque, language: ?[*:0]const u16) callconv(.c) i32 {
    if (name == null) return original_find_package(this, name, guid, out, language);

    var hash = hashName(hashName(0xcbf29ce484222325, name) ^ 0x2f, language);
    if (guid) |g| {
        for (@as(*const [16]u8, @ptrCast(g))) |byte| hash = (hash ^ byte) *% 0x100000001b3;
        hash ^= 0x47;
    }
    hash ^= hash >> 29;
    const slot = &package_misses[@as(usize, @truncate(hash)) & 0xff];
    const tag = hash & ~@as(u64, 0xffffff);
    const now: u64 = (GetTickCount() >> 4) & 0xffffff;

    const seen = slot.load(.monotonic);
    if (seen & ~@as(u64, 0xffffff) == tag and seen != 0 and ((now -% seen) & 0xffffff) < package_miss_ms / 16) {
        _ = package_miss_hits.fetchAdd(1, .monotonic);
        return 0;
    }

    const found = original_find_package(this, name, guid, out, language);
    if (found == 0) slot.store(tag | now, .monotonic);
    return found;
}

// ---------------------------------------------------------------------------
// chunks: the async loader decompresses a whole compressed chunk for every read into it,
// and the script packages are read back and forth while they link: the same few dozen
// chunks are inflated close to two thousand times during a start. A chunk asked for a
// second time is kept, and later reads of it are a copy.
//
// The key carries the identity, size and write time of the file, so a file replaced
// between two reads never answers with old bytes. Everything is released once the cache
// has been idle for a while.

const ReadCompressed = *const fn (this: *const usize, request: usize, handle: *const [2]usize) callconv(.c) usize;

var original_read_compressed: ReadCompressed = undefined;

const ChunkKey = extern struct {
    volume_serial: u32,
    index_high: u32,
    index_low: u32,
    size_high: u32,
    size_low: u32,
    write_time: FILETIME,
    offset: i32,
    size: i32,
    uncompressed_size: i32,
    flags: u32,

    fn eql(a: *const ChunkKey, b: *const ChunkKey) bool {
        return std.mem.eql(u8, std.mem.asBytes(a), std.mem.asBytes(b));
    }

    fn hash(self: *const ChunkKey) u64 {
        const h = std.hash.Wyhash.hash(0x7e57, std.mem.asBytes(self));
        return if (h == 0) 1 else h;
    }
};

const Chunk = struct {
    key: ChunkKey,
    data: ?[*]u8 = null,
    len: usize = 0,
    last_use: u64 = 0,
};

const chunk_budget: usize = 128 << 20;
const chunk_max: usize = 64 << 20;
const chunk_idle_ms: u32 = 20_000;

var chunk_mutex: std.Thread.Mutex = .{};
var chunks: [128]Chunk = undefined;
var chunk_seen: [4096]u64 = @splat(0);
var chunk_bytes: usize = 0;
var chunk_clock: u64 = 0;
var chunk_last_use_ms: u32 = 0;

var chunk_stats: struct {
    hits: u64 = 0,
    misses: u64 = 0,
    stores: u64 = 0,
    evictions: u64 = 0,
    saved_bytes: u64 = 0,
    miss_ticks: i64 = 0,
    reported_hits: u64 = 0,
} = .{};

fn field(comptime T: type, address: usize) T {
    return @as(*align(1) const T, @ptrFromInt(address)).*;
}

fn chunkKey(file: usize, request: usize) ?ChunkKey {
    // 0, or INVALID_HANDLE_VALUE
    if (file == 0 or file == std.math.maxInt(usize)) return null;
    var info: BY_HANDLE_FILE_INFORMATION = undefined;
    if (GetFileInformationByHandle(@ptrFromInt(file), &info) == 0) return null;
    // FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT
    if (info.attributes & 0x410 != 0) return null;
    var key = std.mem.zeroes(ChunkKey);
    key.volume_serial = info.volume_serial;
    key.index_high = info.index_high;
    key.index_low = info.index_low;
    key.size_high = info.size_high;
    key.size_low = info.size_low;
    key.write_time = info.last_write_time;
    // FAsyncIORequest: Offset, Size, UncompressedSize, CompressionFlags
    key.offset = field(i32, request + 0x1c);
    key.size = field(i32, request + 0x20);
    key.uncompressed_size = field(i32, request + 0x24);
    key.flags = field(u32, request + 0x30);
    if (key.offset < 0 or key.size <= 0) return null;
    const file_size = (@as(u64, info.size_high) << 32) | info.size_low;
    if (@as(u64, @intCast(key.offset)) + @as(u64, @intCast(key.size)) > file_size) return null;
    return key;
}

fn chunkRelease(chunk: *Chunk) void {
    if (chunk.data) |data| {
        // MEM_RELEASE
        _ = VirtualFree(data, 0, 0x8000);
        chunk_bytes -= chunk.len;
    }
    chunk.data = null;
    chunk.len = 0;
}

/// Keeps a copy of `data`, evicting the least recently used chunks to stay in budget.
fn chunkStore(key: *const ChunkKey, data: [*]const u8, len: usize) void {
    var slot: ?*Chunk = null;
    while (true) {
        var oldest: ?*Chunk = null;
        for (&chunks) |*chunk| {
            if (chunk.data == null) {
                if (slot == null) slot = chunk;
            } else if (oldest == null or chunk.last_use < oldest.?.last_use) {
                oldest = chunk;
            }
        }
        if (slot != null and chunk_bytes + len <= chunk_budget) break;
        const victim = oldest orelse return;
        chunkRelease(victim);
        chunk_stats.evictions += 1;
        slot = victim;
    }
    // MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE
    const copy = VirtualAlloc(null, len, 0x3000, 0x04) orelse return;
    @memcpy(copy[0..len], data[0..len]);
    chunk_clock += 1;
    slot.?.* = .{ .key = key.*, .data = copy, .len = len, .last_use = chunk_clock };
    chunk_bytes += len;
    chunk_stats.stores += 1;
}

// ---------------------------------------------------------------------------
// inflate: a compressed chunk is a table followed by blocks of up to 128 KiB, each
// compressed on its own. The engine reads and inflates them one after the other. Here
// the chunk is read with one call and its blocks are inflated side by side, with the
// engine's own decompressor. Anything unexpected about the chunk (byte order, sizes that
// do not add up, a block that fails) leaves the whole read to the original, which then
// reports it the way it always did.

const Uncompress = *const fn (flags: u32, dest: [*]u8, dest_len: i32, source: [*]const u8, source_len: i32) callconv(.c) u32;

var uncompress: Uncompress = undefined;
var inflate_ready = false;

const package_tag: u32 = 0x9e2a83c1;
const inflate_max_blocks = 512;
const inflate_max_workers = 6;

const Block = struct { source: u32, source_len: u32, dest: u32, dest_len: u32 };

var inflate_mutex: std.Thread.Mutex = .{};
var inflate_scratch: ?[*]u8 = null;
var inflate_scratch_len: usize = 0;
var inflate_workers: u32 = 0;
var inflate_chunks: u64 = 0;

var job: struct {
    blocks: [inflate_max_blocks]Block = undefined,
    count: u32 = 0,
    flags: u32 = 0,
    source: [*]const u8 = undefined,
    dest: [*]u8 = undefined,
    next: std.atomic.Value(u32) = .init(0),
    done: std.atomic.Value(u32) = .init(0),
    failed: std.atomic.Value(bool) = .init(false),
    open: std.atomic.Value(bool) = .init(false),
    inside: std.atomic.Value(u32) = .init(0),
    generation: std.atomic.Value(u32) = .init(0),
} = .{};

/// The game's packages carry their blocks XORed with one byte.
fn xorBytes(data: []u8, value: u8) void {
    const Lane = @Vector(32, u8);
    const mask: Lane = @splat(value);
    var i: usize = 0;
    while (i + 32 <= data.len) : (i += 32) {
        const lane: Lane = data[i..][0..32].*;
        data[i..][0..32].* = lane ^ mask;
    }
    while (i < data.len) : (i += 1) data[i] ^= value;
}

fn inflateBlocks() void {
    _ = job.inside.fetchAdd(1, .seq_cst);
    defer _ = job.inside.fetchSub(1, .seq_cst);
    if (!job.open.load(.seq_cst)) return;
    while (true) {
        const i = job.next.fetchAdd(1, .monotonic);
        if (i >= job.count) return;
        const block = job.blocks[i];
        const ok = uncompress(job.flags, job.dest + block.dest, @intCast(block.dest_len), job.source + block.source, @intCast(block.source_len));
        if (ok == 0) job.failed.store(true, .monotonic);
        _ = job.done.fetchAdd(1, .release);
    }
}

fn inflateWorker(_: ?*anyopaque) callconv(.winapi) u32 {
    var seen: u32 = 0;
    while (true) {
        std.Thread.Futex.wait(&job.generation, seen);
        seen = job.generation.load(.acquire);
        inflateBlocks();
    }
}

/// Reads and inflates the chunk of `request` into its destination. False when the chunk
/// is left to the original; the destination may then hold a partial result, which the
/// original overwrites.
fn inflateChunk(this: *const usize, request: usize, file: usize) bool {
    if (!inflate_ready or field(u32, base + rva.io_stats_enabled) == 1) return false;
    const flags = field(u32, request + 0x30);
    // COMPRESS_ZLIB or COMPRESS_LZO (what the 8.1 packages use), with or without the XOR.
    // Neither decompressor keeps state between calls.
    if (flags & 0xf != 1 and flags & 0xf != 2) return false;
    const offset = field(i32, request + 0x1c);
    const size: usize = @intCast(field(i32, request + 0x20));
    const uncompressed: u32 = @intCast(field(i32, request + 0x24));
    const dest: [*]u8 = @ptrFromInt(field(usize, request + 0x28));
    if (size < 24) return false;
    // The lock the engine holds around every seek and read of its file handles.
    const lock = field(usize, @intFromPtr(this) + 0x8c);
    if (lock == 0) return false;

    if (!inflate_mutex.tryLock()) return false;
    defer inflate_mutex.unlock();

    if (inflate_scratch_len < size) {
        if (inflate_scratch) |old| _ = VirtualFree(old, 0, 0x8000);
        inflate_scratch_len = 0;
        const wanted = std.mem.alignForward(usize, size, 1 << 20);
        // MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE
        inflate_scratch = VirtualAlloc(null, wanted, 0x3000, 0x04) orelse return false;
        inflate_scratch_len = wanted;
    }
    const scratch = inflate_scratch.?;

    {
        const section: *anyopaque = @ptrFromInt(lock + 8);
        EnterCriticalSection(section);
        defer LeaveCriticalSection(section);
        var read: u32 = 0;
        if (SetFilePointerEx(@ptrFromInt(file), offset, null, 0) == 0) return false;
        if (ReadFile(@ptrFromInt(file), scratch, @intCast(size), &read, null) == 0 or read != size) return false;
    }

    const words: [*]align(1) const u32 = @ptrCast(scratch);
    if (words[0] != package_tag) return false;
    var block_size = words[1];
    if (block_size == package_tag) block_size = 0x20000;
    if (block_size == 0 or block_size > 1 << 24) return false;
    const count = (uncompressed + block_size - 1) / block_size;
    if (count == 0 or count > inflate_max_blocks) return false;
    const header: usize = 16 + @as(usize, count) * 8;
    if (header > size) return false;

    var source: u64 = header;
    var out: u64 = 0;
    for (0..count) |i| {
        const source_len = words[4 + i * 2];
        const dest_len = words[5 + i * 2];
        if (source_len == 0 or source_len > 1 << 30 or dest_len == 0 or dest_len > block_size) return false;
        if (source + source_len > size or out + dest_len > uncompressed) return false;
        job.blocks[i] = .{ .source = @intCast(source), .source_len = source_len, .dest = @intCast(out), .dest_len = dest_len };
        source += source_len;
        out += dest_len;
    }
    // The totals of the table against its rows and against the request.
    if (out != uncompressed or words[3] != uncompressed or words[2] != source - header) return false;

    var job_flags = flags;
    if (flags & 0x200 != 0) {
        xorBytes(scratch[header..@intCast(source)], 0x2a);
        job_flags &= ~@as(u32, 0x200);
    }

    if (inflate_workers == 0 and count > 1) {
        const cpus = std.Thread.getCpuCount() catch 2;
        const wanted: u32 = @intCast(std.math.clamp(cpus / 2, 1, inflate_max_workers));
        while (inflate_workers < wanted) : (inflate_workers += 1) {
            const thread = CreateThread(null, 0, &inflateWorker, null, 0, null) orelse break;
            _ = CloseHandle(thread);
        }
    }

    job.count = count;
    job.flags = job_flags;
    job.source = scratch;
    job.dest = dest;
    job.next.store(0, .monotonic);
    job.done.store(0, .monotonic);
    job.failed.store(false, .monotonic);
    job.open.store(true, .seq_cst);
    if (count > 1) {
        _ = job.generation.fetchAdd(1, .release);
        std.Thread.Futex.wake(&job.generation, @min(count - 1, inflate_workers));
    }
    inflateBlocks();
    while (job.done.load(.acquire) != count) std.atomic.spinLoopHint();
    job.open.store(false, .seq_cst);
    while (job.inside.load(.seq_cst) != 0) _ = SwitchToThread();

    if (job.failed.load(.monotonic)) return false;
    inflate_chunks += 1;
    return true;
}

fn hookReadCompressed(this: *const usize, request: usize, handle: *const [2]usize) callconv(.c) usize {
    const last_error = GetLastError();
    const uncompressed_size = field(i32, request + 0x24);
    const dest = field(usize, request + 0x28);
    // The handle is only a Win32 file handle for the stock platform reader.
    const vtable = this.*;
    const stock_reader = field(usize, vtable + 0x78) == base + rva.platform_read;
    if (!stock_reader or dest == 0 or uncompressed_size <= 0 or uncompressed_size > chunk_max) {
        return original_read_compressed(this, request, handle);
    }
    const len: usize = @intCast(uncompressed_size);
    const key = chunkKey(handle[0], request) orelse {
        SetLastError(last_error);
        return original_read_compressed(this, request, handle);
    };

    {
        chunk_mutex.lock();
        defer chunk_mutex.unlock();
        chunk_last_use_ms = GetTickCount();
        for (&chunks) |*chunk| {
            const data = chunk.data orelse continue;
            if (chunk.len != len or !chunk.key.eql(&key)) continue;
            @memcpy(@as([*]u8, @ptrFromInt(dest))[0..len], data[0..len]);
            chunk_clock += 1;
            chunk.last_use = chunk_clock;
            chunk_stats.hits += 1;
            chunk_stats.saved_bytes += len;
            SetLastError(last_error);
            return 0;
        }
        chunk_stats.misses += 1;
    }

    var started: i64 = 0;
    var ended: i64 = 0;
    _ = QueryPerformanceCounter(&started);
    var result: usize = 0;
    var result_error = last_error;
    if (!inflateChunk(this, request, handle[0])) {
        SetLastError(last_error);
        result = original_read_compressed(this, request, handle);
        result_error = GetLastError();
    }
    _ = QueryPerformanceCounter(&ended);

    {
        chunk_mutex.lock();
        defer chunk_mutex.unlock();
        chunk_stats.miss_ticks += ended - started;
        const hash = key.hash();
        const seen = &chunk_seen[hash % chunk_seen.len];
        if (seen.* == hash) {
            chunkStore(&key, @ptrFromInt(dest), len);
        } else {
            seen.* = hash;
        }
    }

    SetLastError(result_error);
    return result;
}

fn chunkJanitor(_: ?*anyopaque) callconv(.winapi) u32 {
    while (true) {
        Sleep(5000);
        chunk_mutex.lock();
        defer chunk_mutex.unlock();
        if (chunk_stats.hits != chunk_stats.reported_hits) {
            chunk_stats.reported_hits = chunk_stats.hits;
            var frequency: i64 = 1000;
            _ = QueryPerformanceFrequency(&frequency);
            log("chunks: hits={d} misses={d} ({d} ms, {d} inflated here) stores={d} evictions={d} resident={d} MiB, {d} MiB not decompressed again", .{
                chunk_stats.hits,                                             chunk_stats.misses,
                @divTrunc(chunk_stats.miss_ticks * 1000, @max(frequency, 1)), inflate_chunks,
                chunk_stats.stores,                                           chunk_stats.evictions,
                chunk_bytes >> 20,                                            chunk_stats.saved_bytes >> 20,
            });
        }
        if (chunk_bytes != 0 and GetTickCount() -% chunk_last_use_ms > chunk_idle_ms) {
            for (&chunks) |*chunk| chunkRelease(chunk);
            log("packages: {d} repeated misses answered", .{package_miss_hits.load(.monotonic)});
            log("file_size: {d} asked, {d} answered from the last answer", .{ size_memo_misses.load(.monotonic), size_memo_hits.load(.monotonic) });
            if (inflate_mutex.tryLock()) {
                defer inflate_mutex.unlock();
                if (inflate_scratch) |scratch| _ = VirtualFree(scratch, 0, 0x8000);
                inflate_scratch = null;
                inflate_scratch_len = 0;
            }
            log("chunks: idle, memory released", .{});
        }
    }
}

// ---------------------------------------------------------------------------
// graphics: before it opens a window the game creates a DXGI factory to see whether
// Direct3D 11 is there, and a Direct3D 9 object to list the display modes. Both load
// their libraries and the display driver on the main thread. A thread does the same
// while the game is still reading its configuration and keeps the two objects for a
// while, so the game's own calls find everything loaded.

const graphics_hold_ms = 30_000;
const iid_dxgi_factory = [16]u8{ 0xec, 0x66, 0x71, 0x7b, 0xc7, 0x21, 0xae, 0x44, 0xb2, 0x1a, 0xc9, 0xae, 0x32, 0x1a, 0xe3, 0x69 };

/// IUnknown::Release
fn release(object: *anyopaque) void {
    const vtable = @as(*const *const [3]*const fn (*anyopaque) callconv(.winapi) u32, @ptrCast(@alignCast(object))).*;
    _ = vtable[2](object);
}

fn graphicsWarmer(_: ?*anyopaque) callconv(.winapi) u32 {
    const started = GetTickCount();
    var factory: ?*anyopaque = null;
    var d3d9: ?*anyopaque = null;

    if (LoadLibraryA("dxgi.dll")) |dxgi| {
        if (GetProcAddress(dxgi, "CreateDXGIFactory")) |proc| {
            const create: *const fn (*const [16]u8, *?*anyopaque) callconv(.winapi) i32 = @ptrCast(proc);
            if (create(&iid_dxgi_factory, &factory) < 0) factory = null;
        }
    }
    _ = LoadLibraryA("d3d11.dll");
    if (LoadLibraryA("d3d9.dll")) |library| {
        if (GetProcAddress(library, "Direct3DCreate9")) |proc| {
            const create: *const fn (u32) callconv(.winapi) ?*anyopaque = @ptrCast(proc);
            // D3D_SDK_VERSION
            d3d9 = create(32);
        }
    }
    log("graphics: ready after {d} ms (dxgi={} d3d9={})", .{ GetTickCount() -% started, factory != null, d3d9 != null });

    Sleep(graphics_hold_ms);
    if (d3d9) |object| release(object);
    if (factory) |object| release(object);
    return 0;
}

// ---------------------------------------------------------------------------
// crc: the engine's CRC-32 (MSB first, polynomial 0x04C11DB7) goes one byte at a time.
// Slicing by eight gives the same sums several times faster. The eight tables are derived
// from the game's own once it has filled it, and checked against the original function
// before they are used.

const Crc32 = *const fn (data: [*]const u8, len: i32, crc: u32) callconv(.c) u32;

var original_crc32: Crc32 = undefined;
var crc_tables: [8][256]u32 = undefined;
// 0: tables not built yet, 1: in use, 2: the check failed, the original stays
var crc_state = std.atomic.Value(u8).init(0);
var crc_mutex: std.Thread.Mutex = .{};

fn crcFast(data: [*]const u8, len: usize, seed: u32) u32 {
    var crc = ~seed;
    var p = data;
    var left = len;
    while (left >= 8) : ({
        p += 8;
        left -= 8;
    }) {
        const a = std.mem.readInt(u32, p[0..4], .big) ^ crc;
        const b = std.mem.readInt(u32, p[4..8], .big);
        crc = crc_tables[7][a >> 24] ^ crc_tables[6][(a >> 16) & 0xff] ^
            crc_tables[5][(a >> 8) & 0xff] ^ crc_tables[4][a & 0xff] ^
            crc_tables[3][b >> 24] ^ crc_tables[2][(b >> 16) & 0xff] ^
            crc_tables[1][(b >> 8) & 0xff] ^ crc_tables[0][b & 0xff];
    }
    while (left > 0) : ({
        p += 1;
        left -= 1;
    }) {
        crc = (crc << 8) ^ crc_tables[0][(crc >> 24) ^ p[0]];
    }
    return ~crc;
}

fn crcPrepare() void {
    crc_mutex.lock();
    defer crc_mutex.unlock();
    if (crc_state.load(.acquire) != 0) return;

    // Filled by the engine's own start; until then the original has to do.
    const game_table = at(*const [256]u32, rva.crc32_table);
    if (game_table[1] != 0x04C11DB7 or game_table[255] == 0) return;

    crc_tables[0] = game_table.*;
    for (1..8) |k| {
        for (0..256) |i| {
            const previous = crc_tables[k - 1][i];
            crc_tables[k][i] = (previous << 8) ^ crc_tables[0][previous >> 24];
        }
    }

    var sample: [1031]u8 = undefined;
    var random = std.Random.DefaultPrng.init(0x7e3be57);
    random.fill(&sample);
    var ok = true;
    for ([_]usize{ 0, 1, 3 }) |start| {
        for ([_]usize{ 1, 7, 8, 9, 64, 255, 1024 }) |len| {
            for ([_]u32{ 0, 0xdeadbeef }) |seed| {
                const expected = original_crc32(sample[start..].ptr, @intCast(len), seed);
                if (crcFast(sample[start..].ptr, len, seed) != expected) ok = false;
            }
        }
    }
    crc_state.store(if (ok) 1 else 2, .release);
    if (ok) log("crc: tables checked, in use", .{}) else log("crc: check failed, the original stays", .{});
}

fn hookCrc32(data: [*]const u8, len: i32, crc: u32) callconv(.c) u32 {
    if (len < 32) return original_crc32(data, len, crc);
    var state = crc_state.load(.acquire);
    if (state == 0) {
        crcPrepare();
        state = crc_state.load(.acquire);
    }
    if (state != 1) return original_crc32(data, len, crc);
    return crcFast(data, @intCast(len), crc);
}

// ---------------------------------------------------------------------------
// profile: `TEMPEST_FASTSTART_PROFILE=<seconds>` samples the main thread every millisecond
// and logs where it was, two seconds at a time. For finding what to fix next.

const THREADENTRY32 = extern struct {
    size: u32,
    usage: u32,
    thread_id: u32,
    owner_process_id: u32,
    base_priority: i32,
    delta_priority: i32,
    flags: u32,
};

extern "kernel32" fn CreateToolhelp32Snapshot(flags: u32, pid: u32) callconv(.winapi) HANDLE;
extern "kernel32" fn Thread32First(snapshot: HANDLE, entry: *THREADENTRY32) callconv(.winapi) BOOL;
extern "kernel32" fn Thread32Next(snapshot: HANDLE, entry: *THREADENTRY32) callconv(.winapi) BOOL;
extern "kernel32" fn OpenThread(access: u32, inherit: BOOL, id: u32) callconv(.winapi) ?HANDLE;
extern "kernel32" fn GetThreadTimes(thread: HANDLE, creation: *u64, exit: *u64, kernel: *u64, user: *u64) callconv(.winapi) BOOL;
extern "kernel32" fn SuspendThread(thread: HANDLE) callconv(.winapi) u32;
extern "kernel32" fn ResumeThread(thread: HANDLE) callconv(.winapi) u32;
extern "kernel32" fn GetThreadContext(thread: HANDLE, context: *windows.CONTEXT) callconv(.winapi) BOOL;
extern "kernel32" fn RtlLookupFunctionEntry(pc: usize, image_base: *usize, history: ?*anyopaque) callconv(.winapi) ?*RUNTIME_FUNCTION;
extern "kernel32" fn RtlVirtualUnwind(kind: u32, image_base: usize, pc: usize, entry: *RUNTIME_FUNCTION, context: *windows.CONTEXT, handler_data: *?*anyopaque, establisher: *usize, pointers: ?*anyopaque) callconv(.winapi) ?*anyopaque;
extern "kernel32" fn IsBadReadPtr(address: ?*const anyopaque, size: usize) callconv(.winapi) BOOL;

const RUNTIME_FUNCTION = extern struct { BeginAddress: u32, EndAddress: u32, UnwindData: u32 };

var profile_seconds: u32 = 0;

const ProfileBucket = struct { key: u32 = 0, count: u32 = 0 };

/// The thread of the process that was created first.
fn mainThread() ?HANDLE {
    // TH32CS_SNAPTHREAD
    const snapshot = CreateToolhelp32Snapshot(4, 0);
    if (snapshot == invalid_handle) return null;
    defer _ = CloseHandle(snapshot);
    var entry = std.mem.zeroes(THREADENTRY32);
    entry.size = @sizeOf(THREADENTRY32);
    var best: ?HANDLE = null;
    var best_time: u64 = std.math.maxInt(u64);
    if (Thread32First(snapshot, &entry) == 0) return null;
    while (true) {
        if (entry.owner_process_id == GetCurrentProcessId() and entry.thread_id != GetCurrentThreadId()) {
            // THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION
            if (OpenThread(0x2 | 0x8 | 0x40, 0, entry.thread_id)) |thread| {
                var creation: u64 = 0;
                var other: u64 = 0;
                if (GetThreadTimes(thread, &creation, &other, &other, &other) != 0 and creation < best_time) {
                    if (best) |previous| _ = CloseHandle(previous);
                    best = thread;
                    best_time = creation;
                } else {
                    _ = CloseHandle(thread);
                }
            }
        }
        if (Thread32Next(snapshot, &entry) == 0) break;
    }
    return best;
}

fn profileCount(buckets: []ProfileBucket, key: u32) void {
    var slot = (key *% 0x9e3779b1) >> 16;
    while (buckets[slot].key != 0 and buckets[slot].key != key) slot = (slot + 1) & 0xffff;
    buckets[slot].key = key;
    buckets[slot].count += 1;
}

fn profiler(_: ?*anyopaque) callconv(.winapi) u32 {
    const thread = mainThread() orelse return 0;
    const buckets = std.heap.page_allocator.alloc(ProfileBucket, 1 << 16) catch return 0;
    const started = GetTickCount();
    var window: u32 = 0;
    while (window * 2 < profile_seconds) : (window += 1) {
        @memset(buckets, .{});
        var samples: u32 = 0;
        while (GetTickCount() -% started < (window + 1) * 2000) {
            Sleep(1);
            var context: windows.CONTEXT align(16) = undefined;
            context.ContextFlags = 0x100003; // CONTEXT_CONTROL | CONTEXT_INTEGER
            if (SuspendThread(thread) == std.math.maxInt(u32)) continue;
            const ok = GetThreadContext(thread, &context) != 0;
            // Every game function on the stack counts once: inclusive time. The innermost one
            // is also counted as the place the sample was taken (`self`).
            var frames: [64]u32 = undefined;
            var frame_count: usize = 0;
            if (ok) {
                var depth: usize = 0;
                while (depth < 96 and frame_count < frames.len and context.Rip != 0) : (depth += 1) {
                    var image_base: usize = 0;
                    if (RtlLookupFunctionEntry(context.Rip, &image_base, null)) |entry| {
                        if (image_base == base) {
                            frames[frame_count] = entry.BeginAddress;
                            frame_count += 1;
                        }
                        var handler_data: ?*anyopaque = null;
                        var establisher: usize = 0;
                        _ = RtlVirtualUnwind(0, image_base, context.Rip, entry, &context, &handler_data, &establisher, null);
                    } else {
                        if (IsBadReadPtr(@ptrFromInt(context.Rsp), 8) != 0) break;
                        context.Rip = @as(*const usize, @ptrFromInt(context.Rsp)).*;
                        context.Rsp += 8;
                    }
                }
            }
            _ = ResumeThread(thread);
            if (!ok) continue;
            samples += 1;
            for (frames[0..frame_count], 0..) |frame, index| {
                if (std.mem.indexOfScalar(u32, frames[0..index], frame) != null) continue;
                profileCount(buckets, frame | 0x4000_0000);
                if (index == 0) profileCount(buckets, frame | 0x8000_0000);
            }
        }
        log("profile: window {d}-{d}s, {d} samples", .{ window * 2, window * 2 + 2, samples });
        for (0..40) |_| {
            var top: ?*ProfileBucket = null;
            for (buckets) |*bucket| {
                if (bucket.count != 0 and (top == null or bucket.count > top.?.count)) top = bucket;
            }
            const bucket = top orelse break;
            if (bucket.count * 50 < samples) break;
            log("profile:   {d:>5} {s} {x}", .{ bucket.count, if (bucket.key & 0x8000_0000 != 0) "self" else "incl", bucket.key & 0x3fff_ffff });
            bucket.count = 0;
        }
    }
    return 0;
}

// ---------------------------------------------------------------------------

fn attach(comptime name: []const u8, original: anytype, hook: anytype) bool {
    detourz.attach(@ptrCast(original), @constCast(hook)) catch {
        log("{s}: detour failed", .{name});
        return false;
    };
    return true;
}

fn install() void {
    base = @intFromPtr(GetModuleHandleW(null) orelse return);
    if (!isExpectedBuild()) return;

    openLog();
    disabled = disabled_buffer[0..@min(GetEnvironmentVariableA("TEMPEST_FASTSTART_DISABLE", &disabled_buffer, disabled_buffer.len), disabled_buffer.len - 1)];
    log("faststart: Paladins 8.1 Win64 client at {x}", .{base});

    // Waits that have no reason to be as long as they are: the voice client polls for its
    // two ready flags every 100 ms, naming a thread for the debugger sleeps 10 ms first, and
    // every pool worker costs 1 ms more when it is created.
    if (enabled("sleeps")) {
        // mov ecx, 100 -> mov ecx, 1
        patchBytes("sleeps: voice init", rva.voice_init_sleep, &.{ 0xb9, 0x64, 0, 0, 0 }, &.{ 0xb9, 0x01, 0, 0, 0 });
        // mov ecx, 10 -> mov ecx, 0
        patchBytes("sleeps: thread name", rva.thread_name_sleep, &.{ 0xb9, 0x0a, 0, 0, 0 }, &.{ 0xb9, 0x00, 0, 0, 0 });
        // mov ecx, ebp (1) -> xor ecx, ecx
        patchBytes("sleeps: pool worker", rva.pool_worker_sleep, &.{ 0x8b, 0xcd, 0xff, 0x15 }, &.{ 0x33, 0xc9, 0xff, 0x15 });
    }

    const want_rand = enabled("rand") and
        matches(rva.rand_poll, &.{ 0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x10, 0x48, 0x89, 0x7c, 0x24, 0x18, 0x4c }) and
        matches(rva.rand_add, &.{ 0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0xb8, 0x30, 0x00, 0x00, 0x00, 0xe8 });
    const want_file_size = enabled("file_size") and
        matches(rva.file_size, &.{ 0x40, 0x57, 0x48, 0x83, 0xec, 0x40, 0x45, 0x33, 0xc9, 0x48, 0x8b, 0xca, 0x48, 0xc7, 0x44, 0x24 });
    const want_packages = enabled("packages") and
        matches(rva.find_package, &.{ 0x4c, 0x89, 0x4c, 0x24, 0x20, 0x4c, 0x89, 0x44, 0x24, 0x18, 0x48, 0x89, 0x54, 0x24, 0x10, 0x48, 0x89, 0x4c, 0x24, 0x08 });
    const want_chunks = enabled("chunks") and
        matches(rva.read_compressed, &.{ 0x4c, 0x89, 0x44, 0x24, 0x18, 0x55, 0x53, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41 });
    inflate_ready = want_chunks and enabled("inflate") and
        matches(rva.uncompress, &.{ 0x48, 0x8b, 0xc4, 0x48, 0x89, 0x50, 0x10, 0x56, 0x57, 0x48, 0x83, 0xec, 0x48, 0x48, 0x89, 0x68 });
    uncompress = at(Uncompress, rva.uncompress);
    const want_crc = enabled("crc") and
        matches(rva.crc32, &.{ 0x45, 0x33, 0xc9, 0x41, 0xf7, 0xd0, 0x4c, 0x63, 0xd2, 0x85, 0xd2, 0x7e, 0x37, 0x4c, 0x8d, 0x1d, 0xfc, 0xcd, 0x6c, 0x03 });

    original_rand_poll = at(RandPoll, rva.rand_poll);
    rand_add = at(RandAdd, rva.rand_add);
    original_file_size = at(FileSize, rva.file_size);
    original_find_package = at(FindPackage, rva.find_package);
    original_read_compressed = at(ReadCompressed, rva.read_compressed);
    original_crc32 = at(Crc32, rva.crc32);

    for (&chunks) |*chunk| chunk.* = .{ .key = std.mem.zeroes(ChunkKey) };

    detourz.transactionBegin() catch return;
    detourz.updateThread(GetCurrentThread()) catch {};
    const has_rand = want_rand and attach("rand", &original_rand_poll, &hookRandPoll);
    const has_file_size = want_file_size and attach("file_size", &original_file_size, &hookFileSize);
    const has_packages = want_packages and attach("packages", &original_find_package, &hookFindPackage);
    const has_chunks = want_chunks and attach("chunks", &original_read_compressed, &hookReadCompressed);
    const has_crc = want_crc and attach("crc", &original_crc32, &hookCrc32);
    detourz.transactionCommit() catch {
        log("faststart: the detours could not be committed", .{});
        return;
    };
    log("faststart: rand={} file_size={} packages={} chunks={} crc={}", .{ has_rand, has_file_size, has_packages, has_chunks, has_crc });

    var seconds: [16]u8 = undefined;
    const seconds_len = GetEnvironmentVariableA("TEMPEST_FASTSTART_PROFILE", &seconds, seconds.len);
    if (seconds_len > 0 and seconds_len < seconds.len) {
        profile_seconds = std.fmt.parseInt(u32, seconds[0..seconds_len], 10) catch 0;
        if (profile_seconds != 0) {
            if (CreateThread(null, 0, &profiler, null, 0, null)) |thread| _ = CloseHandle(thread);
        }
    }

    if (enabled("graphics")) {
        if (CreateThread(null, 0, &graphicsWarmer, null, 0, null)) |thread| _ = CloseHandle(thread);
    }

    if (has_chunks) {
        if (CreateThread(null, 0, &chunkJanitor, null, 0, null)) |thread| _ = CloseHandle(thread);
    }
}

pub export fn DllMain(inst: windows.HINSTANCE, reason: u32, reserved: ?*anyopaque) windows.BOOL {
    _ = inst;
    _ = reserved;
    // DLL_PROCESS_ATTACH. Never fails the load: an unknown build just gets no patch.
    if (reason == 1) install();
    return windows.TRUE;
}
