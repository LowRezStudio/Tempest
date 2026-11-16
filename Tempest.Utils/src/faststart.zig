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

const WIN32_FIND_DATAW = extern struct {
    attributes: u32,
    creation_time: FILETIME,
    last_access_time: FILETIME,
    last_write_time: FILETIME,
    size_high: u32,
    size_low: u32,
    reserved: [2]u32,
    name: [260]u16,
    short_name: [14]u16,
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
extern "kernel32" fn GetModuleFileNameW(module: ?*anyopaque, buffer: [*]u16, size: u32) callconv(.winapi) u32;
extern "kernel32" fn GetFullPathNameW(name: [*:0]const u16, size: u32, buffer: [*]u16, file_part: ?*?[*]u16) callconv(.winapi) u32;
extern "kernel32" fn FindFirstFileW(pattern: [*:0]const u16, data: *WIN32_FIND_DATAW) callconv(.winapi) HANDLE;
extern "kernel32" fn FindNextFileW(find: HANDLE, data: *WIN32_FIND_DATAW) callconv(.winapi) BOOL;
extern "kernel32" fn FindClose(find: HANDLE) callconv(.winapi) BOOL;
extern "kernel32" fn FindFirstChangeNotificationW(path: [*:0]const u16, subtree: BOOL, filter: u32) callconv(.winapi) HANDLE;
extern "kernel32" fn FindCloseChangeNotification(change: HANDLE) callconv(.winapi) BOOL;
extern "kernel32" fn WaitForSingleObject(handle: HANDLE, ms: u32) callconv(.winapi) u32;
extern "kernel32" fn GetFileAttributesW(name: [*:0]const u16) callconv(.winapi) u32;
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
    const file_size_by_name = 0x127150;
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
    const loader_open_in_profile = 0x18033c;
    const loader_open_in_game = 0x180426;
    const display_query_create = 0xd4e320;
    const audio_settings_sleep = 0x1152b5b;
    const http_get = 0xd773d0;
    const vector_insert = 0x688c0;
    const app_free = 0x154640;
    const max_tick_rate_call = 0xd1ef4c;
    const frame_counter = 0x3866208;
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

/// A page within reach of a 32-bit call displacement from the image, for jumps to this DLL.
var stub_page: ?[*]u8 = null;
var stub_used: usize = 0;

fn nearStub(target: usize) ?usize {
    if (stub_page == null) {
        var address = std.mem.alignForward(usize, base + build_image_size, 0x10000);
        const limit = base + 0x7000_0000;
        while (address < limit and stub_page == null) : (address += 0x10000) {
            // MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE
            stub_page = VirtualAlloc(@ptrFromInt(address), 0x1000, 0x3000, 0x40);
        }
    }
    const page = stub_page orelse return null;
    if (stub_used + 16 > 0x1000) return null;
    const stub = page + stub_used;
    stub_used += 16;
    // jmp [rip+0]; target
    stub[0..6].* = .{ 0xff, 0x25, 0, 0, 0, 0 };
    std.mem.writeInt(u64, stub[6..14], target, .little);
    return @intFromPtr(stub);
}

/// Points one call instruction of the game at `hook`: a `call rel32` (5 bytes) or a
/// `call [rip+disp32]` (6 bytes). `previous` gets what the call reached before, ahead of
/// the change: the function for the first form, the address of the pointer it read for
/// the second.
fn redirectCall(comptime name: []const u8, offset: usize, expected: []const u8, hook: usize, previous: *usize) bool {
    if (!matches(offset, expected)) {
        log("{s}: unexpected bytes, left alone", .{name});
        return false;
    }
    const site = base + offset;
    const displacement: i64 = std.mem.readInt(i32, expected[expected.len - 4 ..][0..4], .little);
    const stub = nearStub(hook) orelse {
        log("{s}: no memory near the image", .{name});
        return false;
    };
    previous.* = @intCast(@as(i64, @intCast(site + expected.len)) + displacement);
    var patch: [6]u8 = .{ 0xe8, 0, 0, 0, 0, 0x90 };
    std.mem.writeInt(i32, patch[1..5], @intCast(@as(i64, @intCast(stub)) - @as(i64, @intCast(site + 5))), .little);
    const code: [*]u8 = @ptrFromInt(site);
    var old: u32 = 0;
    if (VirtualProtect(code, expected.len, 0x40, &old) == 0) {
        log("{s}: cannot unprotect the code page", .{name});
        return false;
    }
    @memcpy(code[0..expected.len], patch[0..expected.len]);
    _ = VirtualProtect(code, expected.len, old, &old);
    _ = FlushInstructionCache(GetCurrentProcess(), code, expected.len);
    log("{s}: patched", .{name});
    return true;
}

/// Replaces the instruction `expected` at `offset` (at least 5 bytes) with a call to `hook`,
/// padded with nops. The hook sees the registers the replaced instruction would have.
fn replaceWithCall(comptime name: []const u8, offset: usize, expected: []const u8, hook: usize) bool {
    if (!matches(offset, expected)) {
        log("{s}: unexpected bytes, left alone", .{name});
        return false;
    }
    const stub = nearStub(hook) orelse {
        log("{s}: no memory near the image", .{name});
        return false;
    };
    var patch: [16]u8 = [_]u8{0x90} ** 16;
    patch[0] = 0xe8;
    std.mem.writeInt(i32, patch[1..5], @intCast(@as(i64, @intCast(stub)) - @as(i64, @intCast(base + offset + 5))), .little);
    patchBytes(name, offset, expected, patch[0..expected.len]);
    return true;
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

/// The size of `path` as `query` reports it, or the answer it gave a moment ago.
fn memoizedSize(comptime query: fn (?*anyopaque, [*:0]const u16) i64, seed: u64, this: ?*anyopaque, path: [*:0]const u16) i64 {
    var hash: u64 = seed;
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

    const size = query(this, path);
    _ = size_memo_misses.fetchAdd(1, .monotonic);
    size_memo_mutex.lock();
    slot.* = .{ .hash = hash, .tick = now, .size = size };
    size_memo_mutex.unlock();
    return size;
}

fn hookFileSize(this: ?*anyopaque, name: ?[*:0]const u16) callconv(.c) i64 {
    const path = name orelse return original_file_size(this, name);
    if (!isFastPath(path)) return original_file_size(this, name);
    return memoizedSize(queryFileSize, 0xcbf29ce484222325, this, path);
}

// The file manager's public size query turns the name into a full path twice (two string
// allocations and a path lookup each) before it gets to the function above. The same
// memo in front of it answers a repeated question without that either.

var original_file_size_by_name: FileSize = undefined;

fn queryFileSizeByName(this: ?*anyopaque, path: [*:0]const u16) i64 {
    return original_file_size_by_name(this, path);
}

fn hookFileSizeByName(this: ?*anyopaque, name: ?[*:0]const u16) callconv(.c) i64 {
    const path = name orelse return original_file_size_by_name(this, name);
    if (!isFastPath(path)) return original_file_size_by_name(this, name);
    return memoizedSize(queryFileSizeByName, 0x9ae16a3b2f90404f, this, path);
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
        opensReport();
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
// opens: every file the async loader reads is opened twice. First under the profile
// folder (Documents\My Games\...), where cooked content never is, then in the game
// folder. When the audio engine starts that happens for each of its sound packages, over
// twelve hundred of them, one after the other, while the main thread waits.
//
// Two things here, both at the loader's two CreateFileW calls and nowhere else:
//
// - The profile folder attempt is answered from a listing of that folder when the name
//   is not in it. The listing is dropped as soon as Windows reports a name added,
//   removed or renamed anywhere under the folder it came from, so a file that appears
//   is found. A name that is in the listing always goes to the file system.
// - A few threads open the sound packages ahead, the same way the loader does, while the
//   game is still loading its script packages. The loader is then handed the handle that
//   is already open: only for the very same full path, once, and whatever it never asks
//   for is closed again.

const OpenSite = enum(u1) { profile, game };

const CreateFile = *const fn (name: [*:0]const u16, access: u32, share: u32, security: ?*anyopaque, disposition: u32, flags: u32, template: ?HANDLE) callconv(.winapi) HANDLE;

/// Where each of the two call sites reads the address of CreateFileW from.
var open_slots: [2]usize = .{ 0, 0 };
var open_counts: [2]std.atomic.Value(u32) = .{ .init(0), .init(0) };
var open_ticks: [2]std.atomic.Value(i64) = .{ .init(0), .init(0) };
var open_reported: u32 = 0;

fn lower(c: u16) u16 {
    return if (c >= 'A' and c <= 'Z') c + ('a' - 'A') else c;
}

fn pathHash(path: []const u16) u64 {
    var hash: u64 = 0xcbf29ce484222325;
    for (path) |c| hash = (hash ^ lower(c)) *% 0x100000001b3;
    return hash;
}

fn pathEql(a: []const u16, b: []const u16) bool {
    if (a.len != b.len) return false;
    for (a, b) |x, y| {
        if (lower(x) != lower(y)) return false;
    }
    return true;
}

/// How the loader opens a file: GENERIC_READ, FILE_SHARE_READ, OPEN_EXISTING,
/// FILE_ATTRIBUTE_NORMAL. Anything else is not one of the opens this is about.
fn isLoaderOpen(access: u32, share: u32, security: ?*anyopaque, disposition: u32, flags: u32, template: ?HANDLE) bool {
    return access == 0x80000000 and share == 1 and security == null and disposition == 3 and flags == 0x80 and template == null;
}

// The listings of the profile folder.

const listing_max_names = 8192;
const listing_max_drops = 24;

const Listing = struct {
    folder: [300]u16 = undefined,
    folder_len: usize = 0,
    /// Signalled by Windows when a name changes under the watched folder.
    watch: ?HANDLE = null,
    exists: bool = false,
    names: []u64 = &.{},
    count: usize = 0,
};

/// Off where change notifications cannot be relied on (see `watchesWork`).
var listing_enabled = false;
var listings: [8]Listing = @splat(.{});
var listing_mutex: std.Thread.Mutex = .{};
var listing_drops: u32 = 0;
var listing_answers = std.atomic.Value(u32).init(0);

fn listingDrop(listing: *Listing) void {
    if (listing.watch) |watch| _ = FindCloseChangeNotification(watch);
    if (listing.names.len != 0) std.heap.page_allocator.free(listing.names);
    listing.* = .{};
    listing_drops += 1;
}

/// The listings stand on Windows telling when a name changes. Wine only does that on
/// Linux, so under Wine anywhere else every open keeps going to the file system.
fn watchesWork() bool {
    const ntdll = GetModuleHandleW(std.unicode.utf8ToUtf16LeStringLiteral("ntdll.dll")) orelse return false;
    const proc = GetProcAddress(ntdll, "wine_get_host_version") orelse return true;
    const host_version: *const fn (?*?[*:0]const u8, ?*?[*:0]const u8) callconv(.c) void = @ptrCast(proc);
    var system: ?[*:0]const u8 = null;
    host_version(&system, null);
    return std.mem.eql(u8, std.mem.span(system orelse return false), "Linux");
}

/// Lists `folder` (a full path ending in a backslash), watching it first so that nothing
/// that changes while it is read goes unnoticed. False when it cannot be watched or is
/// too large: its files then keep going to the file system.
fn listingFill(listing: *Listing, folder: []const u16) bool {
    if (folder.len + 2 > listing.folder.len) return false;

    // The folder itself often does not exist: watch the closest one above it that does.
    var root: [300]u16 = undefined;
    var root_len = folder.len;
    @memcpy(root[0..folder.len], folder);
    while (true) {
        root[root_len] = 0;
        const attributes = GetFileAttributesW(@ptrCast(&root));
        // not INVALID_FILE_ATTRIBUTES, FILE_ATTRIBUTE_DIRECTORY
        if (attributes != std.math.maxInt(u32) and attributes & 0x10 != 0) break;
        if (root_len < 4) return false;
        root_len -= 1;
        while (root_len > 3 and root[root_len - 1] != '\\') root_len -= 1;
    }
    // The whole subtree, FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME
    const watch = FindFirstChangeNotificationW(@ptrCast(&root), 1, 3);
    if (watch == invalid_handle) return false;

    const names = std.heap.page_allocator.alloc(u64, listing_max_names) catch {
        _ = FindCloseChangeNotification(watch);
        return false;
    };
    var count: usize = 0;
    var exists = false;
    var complete = true;

    var pattern: [304]u16 = undefined;
    @memcpy(pattern[0..folder.len], folder);
    pattern[folder.len] = '*';
    pattern[folder.len + 1] = 0;
    var data: WIN32_FIND_DATAW = undefined;
    const find = FindFirstFileW(@ptrCast(&pattern), &data);
    if (find != invalid_handle) {
        exists = true;
        while (true) {
            if (count == names.len) {
                complete = false;
                break;
            }
            names[count] = pathHash(std.mem.sliceTo(&data.name, 0));
            count += 1;
            if (FindNextFileW(find, &data) == 0) break;
        }
        _ = FindClose(find);
    } else {
        const err = GetLastError();
        // ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND
        if (err != 2 and err != 3) complete = false;
    }
    if (!complete) {
        _ = FindCloseChangeNotification(watch);
        std.heap.page_allocator.free(names);
        return false;
    }

    @memcpy(listing.folder[0..folder.len], folder);
    listing.folder_len = folder.len;
    listing.watch = watch;
    listing.exists = exists;
    listing.names = names;
    listing.count = count;
    return true;
}

/// The error CreateFileW would give for `path` (2: no such file, 3: no such folder) when
/// the listing of its folder shows it is not there. Null when the file system has to say.
fn listingMissing(path: []const u16) ?u32 {
    const cut = (std.mem.lastIndexOfScalar(u16, path, '\\') orelse return null) + 1;
    const folder = path[0..cut];
    const name = path[cut..];
    if (name.len == 0 or folder.len < 4) return null;

    if (!listing_enabled) return null;
    listing_mutex.lock();
    defer listing_mutex.unlock();
    // A folder whose names keep changing is not worth listing again and again.
    if (listing_drops > listing_max_drops) return null;

    var found: ?*Listing = null;
    var free: ?*Listing = null;
    for (&listings) |*listing| {
        if (listing.watch == null) {
            if (free == null) free = listing;
        } else if (pathEql(listing.folder[0..listing.folder_len], folder)) {
            found = listing;
        }
    }
    if (found) |listing| {
        // WAIT_OBJECT_0: a name changed since the listing was made
        if (WaitForSingleObject(listing.watch.?, 0) != 258) {
            listingDrop(listing);
            free = listing;
            found = null;
        }
    }
    const listing = found orelse fill: {
        const slot = free orelse return null;
        if (!listingFill(slot, folder)) {
            listing_drops += 1;
            return null;
        }
        break :fill slot;
    };

    if (!listing.exists) return 3;
    const hash = pathHash(name);
    for (listing.names[0..listing.count]) |known| {
        if (known == hash) return null;
    }
    return 2;
}

// The sound packages opened ahead.

const sound_max = 4096;
const sound_hold_ms = 60_000;
const sound_openers = 8;

const SoundFile = struct {
    hash: u64,
    path: []const u16,
    handle: std.atomic.Value(usize) = .init(0),
};

var sound_files: []SoundFile = &.{};
var sound_count: usize = 0;
/// Set once the list is complete; lookups before that go to the file system.
var sound_listed = std.atomic.Value(bool).init(false);
var sound_next = std.atomic.Value(usize).init(0);
var sound_opened = std.atomic.Value(u32).init(0);
var sound_open_ticks = std.atomic.Value(i64).init(0);
var sound_handed = std.atomic.Value(u32).init(0);
var sound_openers_left = std.atomic.Value(u32).init(0);

/// Adds the `.pck` files of `folder` (a full path ending in a backslash) to the list, and
/// its sub-folders one level down when `nested`: those hold the voice lines per language.
fn soundList(arena: std.mem.Allocator, folder: []const u16, nested: bool) void {
    var pattern: [600]u16 = undefined;
    if (folder.len + 2 > pattern.len) return;
    @memcpy(pattern[0..folder.len], folder);
    pattern[folder.len] = '*';
    pattern[folder.len + 1] = 0;
    var data: WIN32_FIND_DATAW = undefined;
    const find = FindFirstFileW(@ptrCast(&pattern), &data);
    if (find == invalid_handle) return;
    defer _ = FindClose(find);
    while (true) {
        const name = std.mem.sliceTo(&data.name, 0);
        // FILE_ATTRIBUTE_DIRECTORY
        if (data.attributes & 0x10 != 0) {
            if (nested and name[0] != '.' and folder.len + name.len + 1 < 500) {
                var sub: [520]u16 = undefined;
                @memcpy(sub[0..folder.len], folder);
                @memcpy(sub[folder.len..][0..name.len], name);
                sub[folder.len + name.len] = '\\';
                soundList(arena, sub[0 .. folder.len + name.len + 1], false);
            }
        } else if (name.len > 4 and sound_count < sound_files.len and
            pathEql(name[name.len - 4 ..], std.unicode.utf8ToUtf16LeStringLiteral(".pck")))
        {
            if (arena.allocSentinel(u16, folder.len + name.len, 0)) |path| {
                @memcpy(path[0..folder.len], folder);
                @memcpy(path[folder.len..], name);
                sound_files[sound_count] = .{ .hash = pathHash(path), .path = path };
                sound_count += 1;
            } else |_| {}
        }
        if (FindNextFileW(find, &data) == 0) break;
    }
}

fn soundOpener(_: ?*anyopaque) callconv(.winapi) u32 {
    const index = std.heap.page_allocator.alloc(u8, 1 << 16) catch null;
    while (true) {
        const i = sound_next.fetchAdd(1, .monotonic);
        if (i >= sound_count) break;
        const entry = &sound_files[i];
        var started: i64 = 0;
        var ended: i64 = 0;
        _ = QueryPerformanceCounter(&started);
        const file = CreateFileW(@ptrCast(entry.path.ptr), 0x80000000, 1, null, 3, 0x80, null);
        if (file == invalid_handle) continue;
        if (index) |buffer| {
            // What the audio engine reads first: "AKPK", the size of the index, the index.
            var read: u32 = 0;
            if (ReadFile(file, buffer.ptr, 8, &read, null) != 0 and read == 8 and
                std.mem.eql(u8, buffer[0..4], "AKPK"))
            {
                const size = std.mem.readInt(u32, buffer[4..8], .little);
                if (size <= buffer.len) _ = ReadFile(file, buffer.ptr, size, &read, null);
            }
            _ = SetFilePointerEx(file, 0, null, 0);
        }
        _ = QueryPerformanceCounter(&ended);
        _ = sound_open_ticks.fetchAdd(ended - started, .monotonic);
        entry.handle.store(@intFromPtr(file), .release);
        _ = sound_opened.fetchAdd(1, .monotonic);
    }
    if (index) |buffer| std.heap.page_allocator.free(buffer);
    _ = sound_openers_left.fetchSub(1, .release);
    return 0;
}

fn ticksToMs(ticks: i64) i64 {
    var frequency: i64 = 1000;
    _ = QueryPerformanceFrequency(&frequency);
    return @divTrunc(ticks * 1000, @max(frequency, 1));
}

fn soundWarmer(_: ?*anyopaque) callconv(.winapi) u32 {
    const started = GetTickCount();
    var arena_state = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    const arena = arena_state.allocator();
    sound_files = arena.alloc(SoundFile, sound_max) catch return 0;

    // <game>\Binaries\Win64\<exe> -> <game>\ChaosGame\CookedPCConsole\
    var folder: [520]u16 = undefined;
    var len: usize = GetModuleFileNameW(null, &folder, 400);
    if (len == 0 or len >= 400) return 0;
    var cut: u32 = 0;
    while (len > 0 and cut < 3) {
        len -= 1;
        if (folder[len] == '\\') cut += 1;
    }
    if (cut != 3) return 0;
    const tail = std.unicode.utf8ToUtf16LeStringLiteral("\\ChaosGame\\CookedPCConsole\\");
    @memcpy(folder[len..][0..tail.len], tail);
    soundList(arena, folder[0 .. len + tail.len], true);
    sound_listed.store(true, .release);

    const cpus = std.Thread.getCpuCount() catch 2;
    const openers: u32 = @intCast(std.math.clamp(cpus, 2, sound_openers));
    sound_openers_left.store(openers, .release);
    for (0..openers) |_| {
        if (CreateThread(null, 0, &soundOpener, null, 0, null)) |thread| {
            _ = CloseHandle(thread);
        } else {
            _ = sound_openers_left.fetchSub(1, .release);
        }
    }
    while (sound_openers_left.load(.acquire) != 0) Sleep(5);
    log("opens: {d} of {d} sound packages opened ahead in {d} ms ({d} ms of opening over {d} threads)", .{
        sound_opened.load(.monotonic), sound_count, GetTickCount() -% started, ticksToMs(sound_open_ticks.load(.monotonic)), openers,
    });

    Sleep(sound_hold_ms);
    sound_listed.store(false, .release);
    var closed: u32 = 0;
    for (sound_files[0..sound_count]) |*entry| {
        const handle = entry.handle.swap(0, .acq_rel);
        if (handle == 0) continue;
        _ = CloseHandle(@ptrFromInt(handle));
        closed += 1;
    }
    log("opens: {d} sound packages handed to the game, {d} never asked for and closed", .{ sound_handed.load(.monotonic), closed });
    return 0;
}

/// The handle opened ahead for this path, if there is one that nobody took yet.
fn soundTake(path: []const u16) ?HANDLE {
    if (!sound_listed.load(.acquire)) return null;
    const hash = pathHash(path);
    for (sound_files[0..sound_count]) |*entry| {
        if (entry.hash != hash or !pathEql(entry.path, path)) continue;
        const handle = entry.handle.swap(0, .acq_rel);
        return if (handle == 0) null else @ptrFromInt(handle);
    }
    return null;
}

fn loaderOpen(site: OpenSite, name: [*:0]const u16, access: u32, share: u32, security: ?*anyopaque, disposition: u32, flags: u32, template: ?HANDLE) HANDLE {
    if (isLoaderOpen(access, share, security, disposition, flags, template)) {
        var full: [600]u16 = undefined;
        const len = GetFullPathNameW(name, full.len, &full, null);
        if (len != 0 and len < full.len) {
            const path = full[0..len];
            switch (site) {
                .profile => if (listingMissing(path)) |err| {
                    _ = listing_answers.fetchAdd(1, .monotonic);
                    SetLastError(err);
                    return invalid_handle;
                },
                .game => if (soundTake(path)) |handle| {
                    _ = sound_handed.fetchAdd(1, .monotonic);
                    SetLastError(0);
                    return handle;
                },
            }
        }
    }
    const original = @as(*const CreateFile, @ptrFromInt(open_slots[@intFromEnum(site)])).*;
    var started: i64 = 0;
    var ended: i64 = 0;
    _ = QueryPerformanceCounter(&started);
    const file = original(name, access, share, security, disposition, flags, template);
    const result_error = GetLastError();
    _ = QueryPerformanceCounter(&ended);
    _ = open_counts[@intFromEnum(site)].fetchAdd(1, .monotonic);
    _ = open_ticks[@intFromEnum(site)].fetchAdd(ended - started, .monotonic);
    SetLastError(result_error);
    return file;
}

fn hookOpenInProfile(name: [*:0]const u16, access: u32, share: u32, security: ?*anyopaque, disposition: u32, flags: u32, template: ?HANDLE) callconv(.winapi) HANDLE {
    return loaderOpen(.profile, name, access, share, security, disposition, flags, template);
}

fn hookOpenInGame(name: [*:0]const u16, access: u32, share: u32, security: ?*anyopaque, disposition: u32, flags: u32, template: ?HANDLE) callconv(.winapi) HANDLE {
    return loaderOpen(.game, name, access, share, security, disposition, flags, template);
}

fn opensReport() void {
    const total = open_counts[0].load(.monotonic) + open_counts[1].load(.monotonic) + listing_answers.load(.monotonic);
    if (total == open_reported) return;
    open_reported = total;
    log("opens: profile folder {d} answered from its listing, {d} asked of the file system ({d} ms); game folder {d} opened ({d} ms), {d} handed over", .{
        listing_answers.load(.monotonic),
        open_counts[0].load(.monotonic),
        ticksToMs(open_ticks[0].load(.monotonic)),
        open_counts[1].load(.monotonic),
        ticksToMs(open_ticks[1].load(.monotonic)),
        sound_handed.load(.monotonic),
    });
}

// ---------------------------------------------------------------------------
// graphics: before it opens a window the game creates a DXGI factory to see whether
// Direct3D 11 is there, and a Direct3D 9 object to list the display modes. Both load
// their libraries and the display driver on the main thread. A thread does the same
// while the game is still reading its configuration and keeps the two objects for a
// while, so the game's own calls find everything loaded.
//
// The display mode query gets more than a warm library: it is handed the Direct3D 9
// object this thread already made (with a reference of its own, which it releases as it
// always did), so it does not create a second one. Its answers still come from Direct3D.

const graphics_hold_ms = 30_000;
const iid_dxgi_factory = [16]u8{ 0xec, 0x66, 0x71, 0x7b, 0xc7, 0x21, 0xae, 0x44, 0xb2, 0x1a, 0xc9, 0xae, 0x32, 0x1a, 0xe3, 0x69 };
// D3DFMT_X8R8G8B8, the format the game lists modes for
const display_format = 0x16;

const CreateD3D9 = *const fn (sdk: u32) callconv(.winapi) ?*anyopaque;

var graphics_mutex: std.Thread.Mutex = .{};
var shared_d3d9: ?*anyopaque = null;
var original_create_d3d9: CreateD3D9 = undefined;
var display_shares = std.atomic.Value(u32).init(0);

fn method(object: *anyopaque, comptime index: usize, comptime T: type) T {
    const vtable = @as(*const [*]const usize, @ptrCast(@alignCast(object))).*;
    return @ptrFromInt(vtable[index]);
}

/// IUnknown::Release
fn release(object: *anyopaque) void {
    _ = method(object, 2, *const fn (*anyopaque) callconv(.winapi) u32)(object);
}

fn hookCreateD3D9(sdk: u32) callconv(.winapi) ?*anyopaque {
    {
        graphics_mutex.lock();
        defer graphics_mutex.unlock();
        if (shared_d3d9) |object| {
            // IUnknown::AddRef
            _ = method(object, 1, *const fn (*anyopaque) callconv(.winapi) u32)(object);
            if (display_shares.fetchAdd(1, .monotonic) == 0) log("graphics: display mode query uses the Direct3D 9 object made ahead", .{});
            return object;
        }
    }
    return original_create_d3d9(sdk);
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
    // Only the library: a device made here while the game makes its own can make the
    // game's attempt fail, and it then falls back to Direct3D 9.
    _ = LoadLibraryA("d3d11.dll");
    if (LoadLibraryA("d3d9.dll")) |library| {
        if (GetProcAddress(library, "Direct3DCreate9")) |proc| {
            const create: CreateD3D9 = @ptrCast(proc);
            // D3D_SDK_VERSION
            d3d9 = create(32);
        }
    }
    var modes: u32 = 0;
    if (d3d9) |object| {
        // The game's questions, asked once here so the answers are at hand:
        // GetAdapterIdentifier, GetAdapterModeCount, EnumAdapterModes, GetAdapterDisplayMode.
        var identifier: [2048]u8 align(8) = undefined;
        var mode: [4]u32 = undefined;
        _ = method(object, 5, *const fn (*anyopaque, u32, u32, *anyopaque) callconv(.winapi) i32)(object, 0, 0, &identifier);
        modes = method(object, 6, *const fn (*anyopaque, u32, u32) callconv(.winapi) u32)(object, 0, display_format);
        for (0..modes) |i| {
            _ = method(object, 7, *const fn (*anyopaque, u32, u32, u32, *[4]u32) callconv(.winapi) i32)(object, 0, display_format, @intCast(i), &mode);
        }
        _ = method(object, 8, *const fn (*anyopaque, u32, *[4]u32) callconv(.winapi) i32)(object, 0, &mode);
        graphics_mutex.lock();
        shared_d3d9 = object;
        graphics_mutex.unlock();
    }
    log("graphics: ready after {d} ms (dxgi={} d3d9={} modes={d})", .{ GetTickCount() -% started, factory != null, d3d9 != null, modes });

    Sleep(graphics_hold_ms);
    graphics_mutex.lock();
    shared_d3d9 = null;
    graphics_mutex.unlock();
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
var profile_window_ms: u32 = 2000;

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

extern "kernel32" fn GetModuleHandleExW(flags: u32, address: usize, module: *?*anyopaque) callconv(.winapi) BOOL;

var profile_modules: [64]usize = [_]usize{0} ** 64;
var profile_module_names: [64][64]u8 = undefined;
var profile_module_name_lens: [64]usize = [_]usize{0} ** 64;

/// Slot of the module holding `address`; the last slot collects code outside any module.
fn profileModuleSlot(address: usize) u32 {
    const unknown = profile_modules.len - 1;
    var module: ?*anyopaque = null;
    // GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT
    if (GetModuleHandleExW(4 | 2, address, &module) == 0 or module == null) return unknown;
    const handle = @intFromPtr(module.?);
    for (profile_modules[0..unknown], 0..) |known, slot| {
        if (known == handle) return @intCast(slot);
        if (known != 0) continue;
        profile_modules[slot] = handle;
        var path: [260]u16 = undefined;
        const n = GetModuleFileNameW(module, &path, path.len);
        var start: usize = 0;
        for (path[0..n], 0..) |c, i| {
            if (c == '\\' or c == '/') start = i + 1;
        }
        var len: usize = 0;
        for (path[start..n]) |c| {
            if (len == profile_module_names[slot].len) break;
            profile_module_names[slot][len] = if (c < 0x80) @intCast(c) else '?';
            len += 1;
        }
        profile_module_name_lens[slot] = len;
        return @intCast(slot);
    }
    return unknown;
}

fn profileIsSystem(slot: u32) bool {
    const name = profileModuleName(slot);
    for ([_][]const u8{ "ntdll.dll", "KERNELBASE.dll", "KERNEL32.DLL", "win32u.dll", "cryptnet.dll", "CRYPT32.dll", "WINTRUST.dll", "mswsock.dll", "WS2_32.dll", "RPCRT4.dll", "combase.dll", "(no module)" }) |system| {
        if (std.ascii.eqlIgnoreCase(name, system)) return true;
    }
    return false;
}

fn profileModuleName(slot: u32) []const u8 {
    if (slot >= profile_modules.len - 1 or profile_modules[slot] == 0) return "(no module)";
    return profile_module_names[slot][0..profile_module_name_lens[slot]];
}

extern "kernel32" fn GetThreadId(thread: HANDLE) callconv(.winapi) u32;

var profile_busiest = false;
var busy_ids: [512]u32 = undefined;
var busy_times: [512]u64 = undefined;
var busy_count: usize = 0;

/// With `TEMPEST_FASTSTART_PROFILE_THREAD=busiest`: the thread other than the main one that
/// used the most CPU since the last call (the render thread, while the game waits on it).
fn busiestThread(main_id: u32) ?HANDLE {
    const snapshot = CreateToolhelp32Snapshot(4, 0);
    if (snapshot == invalid_handle) return null;
    defer _ = CloseHandle(snapshot);
    var entry = std.mem.zeroes(THREADENTRY32);
    entry.size = @sizeOf(THREADENTRY32);
    var ids: [512]u32 = undefined;
    var times: [512]u64 = undefined;
    var count: usize = 0;
    var best: ?HANDLE = null;
    var best_delta: u64 = 0;
    var ok = Thread32First(snapshot, &entry) != 0;
    while (ok) : (ok = Thread32Next(snapshot, &entry) != 0) {
        if (entry.owner_process_id != GetCurrentProcessId() or entry.thread_id == main_id or entry.thread_id == GetCurrentThreadId()) continue;
        const thread = OpenThread(0x2 | 0x8 | 0x40, 0, entry.thread_id) orelse continue;
        var creation: u64 = 0;
        var exit: u64 = 0;
        var kernel: u64 = 0;
        var user: u64 = 0;
        if (GetThreadTimes(thread, &creation, &exit, &kernel, &user) == 0 or count == ids.len) {
            _ = CloseHandle(thread);
            continue;
        }
        const total = kernel + user;
        var previous: u64 = 0;
        for (busy_ids[0..busy_count], busy_times[0..busy_count]) |id, time| {
            if (id == entry.thread_id) previous = time;
        }
        ids[count] = entry.thread_id;
        times[count] = total;
        count += 1;
        if (total - previous > best_delta) {
            if (best) |old| _ = CloseHandle(old);
            best = thread;
            best_delta = total - previous;
        } else {
            _ = CloseHandle(thread);
        }
    }
    @memcpy(busy_ids[0..count], ids[0..count]);
    @memcpy(busy_times[0..count], times[0..count]);
    busy_count = count;
    return best;
}

fn profiler(_: ?*anyopaque) callconv(.winapi) u32 {
    const main = mainThread() orelse return 0;
    var thread = main;
    const buckets = std.heap.page_allocator.alloc(ProfileBucket, 1 << 16) catch return 0;
    const started = GetTickCount();
    var window: u32 = 0;
    while (window * profile_window_ms < profile_seconds * 1000) : (window += 1) {
        @memset(buckets, .{});
        var samples: u32 = 0;
        if (profile_busiest) {
            if (thread != main) _ = CloseHandle(thread);
            thread = busiestThread(GetThreadId(main)) orelse main;
            log("profile: window {d} samples thread {d}", .{ window, GetThreadId(thread) });
        }
        while (GetTickCount() -% started < (window + 1) * profile_window_ms) {
            Sleep(1);
            var context: windows.CONTEXT align(16) = undefined;
            context.ContextFlags = 0x100003; // CONTEXT_CONTROL | CONTEXT_INTEGER
            if (SuspendThread(thread) == std.math.maxInt(u32)) continue;
            const ok = GetThreadContext(thread, &context) != 0;
            // Every game function on the stack counts once: inclusive time. The innermost one
            // is also counted as the place the sample was taken (`self`).
            var frames: [64]u32 = undefined;
            var frame_count: usize = 0;
            // A sample outside the game (a system call, an injected DLL's detour) counts
            // against that module rather than as the innermost game function's own time.
            var outside_rip: ?usize = null;
            // Where the innermost game function was when the sample was taken: the return
            // address of its call out, when the thread was outside the game.
            var first_site: u32 = 0;
            // Return addresses outside the game above the innermost game frame, innermost first.
            var chain: [8]usize = undefined;
            var chain_len: usize = 0;
            if (ok) {
                var image: usize = 0;
                if (RtlLookupFunctionEntry(context.Rip, &image, null) == null or image != base) outside_rip = context.Rip;
            }
            if (ok) {
                var depth: usize = 0;
                while (depth < 96 and frame_count < frames.len and context.Rip != 0) : (depth += 1) {
                    if (frame_count == 0 and outside_rip != null and chain_len < chain.len) {
                        var image: usize = 0;
                        if (RtlLookupFunctionEntry(context.Rip, &image, null) == null or image != base) {
                            chain[chain_len] = context.Rip;
                            chain_len += 1;
                        }
                    }
                    var image_base: usize = 0;
                    if (RtlLookupFunctionEntry(context.Rip, &image_base, null)) |entry| {
                        if (image_base == base) {
                            // appSleep says nothing on its own: name who called it.
                            if (frame_count == 0 or (frame_count == 1 and frames[0] == 0x1fc280)) first_site = @intCast(context.Rip - base);
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
            // After the resume: the module lookup takes the loader lock the thread may hold.
            const outside: ?u32 = if (outside_rip) |rip| profileModuleSlot(rip) else null;
            samples += 1;
            if (outside) |slot| {
                profileCount(buckets, slot | 0x2000_0000);
                // The game function that called out, for telling waits and detours apart.
                if (frame_count > 0) profileCount(buckets, first_site | 0x1000_0000);
                // The innermost frame of a module that is not part of Windows: whose code
                // made the call that is waiting or working.
                // Also the next three such frames, which tell apart the callers of a shared helper.
                var named: u32 = 0;
                for (chain[0..chain_len]) |rip| {
                    const owner = profileModuleSlot(rip);
                    if (profileIsSystem(owner)) continue;
                    const offset = (rip - profile_modules[owner]) >> 4;
                    if (offset <= 0xfffff and owner < 0x40) profileCount(buckets, (if (named == 0) @as(u32, 0x0800_0000) else 0x0400_0000) | (owner << 20) | @as(u32, @intCast(offset)));
                    named += 1;
                    if (named == 4) break;
                }
            }
            for (frames[0..frame_count], 0..) |frame, index| {
                if (std.mem.indexOfScalar(u32, frames[0..index], frame) != null) continue;
                profileCount(buckets, frame | 0x4000_0000);
                if (index == 0 and outside == null) profileCount(buckets, frame | 0x8000_0000);
            }
        }
        log("profile: window {d}-{d} ms, {d} samples", .{ window * profile_window_ms, (window + 1) * profile_window_ms, samples });
        for (0..40) |_| {
            var top: ?*ProfileBucket = null;
            for (buckets) |*bucket| {
                if (bucket.count != 0 and (top == null or bucket.count > top.?.count)) top = bucket;
            }
            const bucket = top orelse break;
            if (bucket.count * 50 < samples) break;
            if (bucket.key & 0x2000_0000 != 0) {
                log("profile:   {d:>5} in   {s}", .{ bucket.count, profileModuleName(bucket.key & 0xff) });
            } else if (bucket.key & 0xf000_0000 == 0 and bucket.key & 0x0c00_0000 != 0) {
                const kind = if (bucket.key & 0x0800_0000 != 0) "by  " else "via ";
                log("profile:   {d:>5} {s} {s}+{x}", .{ bucket.count, kind, profileModuleName((bucket.key >> 20) & 0x3f), (bucket.key & 0xfffff) << 4 });
            } else {
                const kind = if (bucket.key & 0x8000_0000 != 0) "self" else if (bucket.key & 0x1000_0000 != 0) "out " else "incl";
                log("profile:   {d:>5} {s} {x}", .{ bucket.count, kind, bucket.key & 0x0fff_ffff });
            }
            bucket.count = 0;
        }
    }
    return 0;
}

// ---------------------------------------------------------------------------
// login_frames: the engine loop tells the login screen it is ready on frame 60
// (`GFrameCounter == 60`), and the frame limiter paces those frames, so the login screen
// waited about a second with nothing to do. Until then the limiter is told there is no
// maximum tick rate; the same frames run, only without the sleeps between them.

const login_ready_frame = 60;

const GetMaxTickRate = *const fn (engine: *anyopaque, delta: f32, allow_smoothing: u32) callconv(.c) f32;

fn hookMaxTickRate(engine: *anyopaque, delta: f32, allow_smoothing: u32) callconv(.c) f32 {
    const frames: *const volatile u64 = @ptrFromInt(base + rva.frame_counter);
    if (frames.* < login_ready_frame) return 0;
    const vtable: *const [*]const GetMaxTickRate = @ptrCast(@alignCast(engine));
    return vtable.*[0x2a0 / 8](engine, delta, allow_smoothing);
}

// ---------------------------------------------------------------------------
// discovery: the first thing the engine does is ask discovery.hirez.net where the login
// servers are, a fresh HTTPS connection to another continent (0.3 to 0.6 s) while the game
// thread waits. Its answer is configuration that rarely changes, so the first request of a
// start is answered from the answer saved by the previous start, and a background thread
// fetches the current one for the next start. Without a saved answer (first start, other
// game version) the game asks the server itself, as before.

const FString = extern struct { data: ?[*]u16, num: i32, max: i32 };
const ByteVector = extern struct { first: ?[*]u8, last: ?[*]u8, end: ?[*]u8 };
const HttpGet = *const fn (context: *anyopaque, url: *FString, out: *ByteVector) callconv(.c) u32;
const VectorInsert = *const fn (vector: *ByteVector, at: ?[*]u8, first: [*]const u8, last: [*]const u8, tag: u8) callconv(.c) void;
const AppFree = *const fn (memory: ?*anyopaque) callconv(.c) void;

var original_http_get: HttpGet = undefined;
var discovery_first = std.atomic.Value(bool).init(true);
var discovery_url_buffer: [512]u16 = undefined;
var discovery_url: []const u16 = &.{};

fn discoveryCachePath(buffer: *[300]u16) ?[*:0]const u16 {
    const n = GetTempPathW(260, buffer);
    if (n == 0 or n > 260) return null;
    const name = std.unicode.utf8ToUtf16LeStringLiteral("tempest-discovery.cache");
    @memcpy(buffer[n .. n + name.len], name);
    buffer[n + name.len] = 0;
    return @ptrCast(buffer);
}

/// The saved answer for `url`: the file holds the URL (UTF-16, NUL-terminated), then the body.
fn readDiscoveryCache(url: []const u16, body: []u8) ?usize {
    var path: [300]u16 = undefined;
    const name = discoveryCachePath(&path) orelse return null;
    // GENERIC_READ, FILE_SHARE_READ, OPEN_EXISTING
    const file = CreateFileW(name, 0x80000000, 1, null, 3, 0x80, null);
    if (file == invalid_handle) return null;
    defer _ = CloseHandle(file);
    var raw: [16384]u8 = undefined;
    var len: u32 = 0;
    if (ReadFile(file, &raw, raw.len, &len, null) == 0) return null;
    const key_bytes = (url.len + 1) * 2;
    if (len <= key_bytes or len - key_bytes > body.len) return null;
    if (!std.mem.eql(u8, raw[0 .. url.len * 2], std.mem.sliceAsBytes(url))) return null;
    if (raw[url.len * 2] != 0 or raw[url.len * 2 + 1] != 0) return null;
    const saved = raw[key_bytes..len];
    if (saved.len == 0 or saved[0] != '{') return null;
    @memcpy(body[0..saved.len], saved);
    return saved.len;
}

fn writeDiscoveryCache(url: []const u16, body: []const u8) void {
    if (body.len == 0 or body[0] != '{' or body.len > 8192) return;
    var path: [300]u16 = undefined;
    const name = discoveryCachePath(&path) orelse return;
    // GENERIC_WRITE, CREATE_ALWAYS
    const file = CreateFileW(name, 0x40000000, 0, null, 2, 0x80, null);
    if (file == invalid_handle) return;
    defer _ = CloseHandle(file);
    const zero = [2]u8{ 0, 0 };
    _ = WriteFile(file, std.mem.sliceAsBytes(url).ptr, @intCast(url.len * 2), null, null);
    _ = WriteFile(file, &zero, 2, null, null);
    _ = WriteFile(file, body.ptr, @intCast(body.len), null, null);
}

const WinHttpOpenFn = *const fn (agent: ?[*:0]const u16, access: u32, proxy: ?[*:0]const u16, bypass: ?[*:0]const u16, flags: u32) callconv(.winapi) ?*anyopaque;
const WinHttpConnectFn = *const fn (session: *anyopaque, host: [*:0]const u16, port: u16, reserved: u32) callconv(.winapi) ?*anyopaque;
const WinHttpOpenRequestFn = *const fn (connect: *anyopaque, verb: [*:0]const u16, path: [*:0]const u16, version: ?[*:0]const u16, referrer: ?[*:0]const u16, accept: ?*const ?[*:0]const u16, flags: u32) callconv(.winapi) ?*anyopaque;
const WinHttpSendRequestFn = *const fn (request: *anyopaque, headers: ?[*]const u16, headers_len: u32, optional: ?*anyopaque, optional_len: u32, total: u32, context: usize) callconv(.winapi) BOOL;
const WinHttpReceiveResponseFn = *const fn (request: *anyopaque, reserved: ?*anyopaque) callconv(.winapi) BOOL;
const WinHttpQueryHeadersFn = *const fn (request: *anyopaque, info: u32, name: ?[*:0]const u16, buffer: *anyopaque, len: *u32, index: ?*u32) callconv(.winapi) BOOL;
const WinHttpReadDataFn = *const fn (request: *anyopaque, buffer: [*]u8, len: u32, read: *u32) callconv(.winapi) BOOL;
const WinHttpCloseHandleFn = *const fn (handle: *anyopaque) callconv(.winapi) BOOL;

extern "kernel32" fn LoadLibraryW(name: [*:0]const u16) callconv(.winapi) ?*anyopaque;

/// Fetches the current answer with WinHTTP (not the game's client, which is not safe to
/// run beside the game thread) and saves it for the next start.
fn refreshDiscovery(_: ?*anyopaque) callconv(.winapi) u32 {
    const url = discovery_url;
    const scheme = std.unicode.utf8ToUtf16LeStringLiteral("https://");
    if (url.len <= scheme.len or !std.mem.eql(u16, url[0..scheme.len], scheme)) return 0;
    const rest = url[scheme.len..];
    const slash = std.mem.indexOfScalar(u16, rest, '/') orelse return 0;
    var host: [256:0]u16 = undefined;
    var path: [512:0]u16 = undefined;
    if (slash >= host.len or rest.len - slash >= path.len) return 0;
    @memcpy(host[0..slash], rest[0..slash]);
    host[slash] = 0;
    @memcpy(path[0 .. rest.len - slash], rest[slash..]);
    path[rest.len - slash] = 0;

    const winhttp = LoadLibraryW(std.unicode.utf8ToUtf16LeStringLiteral("winhttp.dll")) orelse return 0;
    const open: WinHttpOpenFn = @ptrCast(GetProcAddress(winhttp, "WinHttpOpen") orelse return 0);
    const connect: WinHttpConnectFn = @ptrCast(GetProcAddress(winhttp, "WinHttpConnect") orelse return 0);
    const open_request: WinHttpOpenRequestFn = @ptrCast(GetProcAddress(winhttp, "WinHttpOpenRequest") orelse return 0);
    const send: WinHttpSendRequestFn = @ptrCast(GetProcAddress(winhttp, "WinHttpSendRequest") orelse return 0);
    const receive: WinHttpReceiveResponseFn = @ptrCast(GetProcAddress(winhttp, "WinHttpReceiveResponse") orelse return 0);
    const query: WinHttpQueryHeadersFn = @ptrCast(GetProcAddress(winhttp, "WinHttpQueryHeaders") orelse return 0);
    const read: WinHttpReadDataFn = @ptrCast(GetProcAddress(winhttp, "WinHttpReadData") orelse return 0);
    const close: WinHttpCloseHandleFn = @ptrCast(GetProcAddress(winhttp, "WinHttpCloseHandle") orelse return 0);

    // WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY
    const session = open(null, 4, null, null, 0) orelse return 0;
    defer _ = close(session);
    const connection = connect(session, &host, 443, 0) orelse return 0;
    defer _ = close(connection);
    // WINHTTP_FLAG_SECURE
    const request = open_request(connection, std.unicode.utf8ToUtf16LeStringLiteral("GET"), &path, null, null, null, 0x00800000) orelse return 0;
    defer _ = close(request);
    if (send(request, null, 0, null, 0, 0, 0) == 0 or receive(request, null) == 0) return 0;
    var status: u32 = 0;
    var status_len: u32 = @sizeOf(u32);
    // WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER
    if (query(request, 19 | 0x20000000, null, &status, &status_len, null) == 0 or status != 200) return 0;
    var body: [8192]u8 = undefined;
    var len: usize = 0;
    while (len < body.len) {
        var got: u32 = 0;
        if (read(request, body[len..].ptr, @intCast(body.len - len), &got) == 0) return 0;
        if (got == 0) break;
        len += got;
    }
    if (len == body.len) return 0;
    writeDiscoveryCache(url, body[0..len]);
    log("discovery: saved the current answer ({d} bytes) for the next start", .{len});
    return 0;
}

fn hookHttpGet(context: *anyopaque, url: *FString, out: *ByteVector) callconv(.c) u32 {
    if (!discovery_first.swap(false, .acq_rel)) return original_http_get(context, url, out);
    const data = url.data orelse return original_http_get(context, url, out);
    const url_len: usize = @intCast(@max(url.num - 1, 0));
    if (url_len == 0 or url_len > discovery_url_buffer.len) return original_http_get(context, url, out);
    @memcpy(discovery_url_buffer[0..url_len], data[0..url_len]);
    discovery_url = discovery_url_buffer[0..url_len];

    var body: [8192]u8 = undefined;
    const saved = readDiscoveryCache(discovery_url, &body) orelse {
        const result = original_http_get(context, url, out);
        if (result != 0) {
            if (out.first) |first| writeDiscoveryCache(discovery_url, first[0 .. @intFromPtr(out.last) - @intFromPtr(first)]);
            log("discovery: asked the server (nothing saved yet), saved its answer", .{});
        }
        return result;
    };
    if (out.first != out.last) return original_http_get(context, url, out);
    const insert: VectorInsert = at(VectorInsert, rva.vector_insert);
    insert(out, out.first, &body, body[saved..].ptr, 0);
    // The URL is passed by value: the callee frees it, as the original does on return.
    url.num = 0;
    url.max = 0;
    at(AppFree, rva.app_free)(url.data);
    url.data = null;
    log("discovery: answered from the previous start ({d} bytes)", .{saved});
    if (CreateThread(null, 0, &refreshDiscovery, null, 0, null)) |thread| _ = CloseHandle(thread);
    return 1;
}

// ---------------------------------------------------------------------------
// browser_trust:Coherent UI (the in-game browser) checks the Authenticode signature of
// its host program when it starts, and Windows fetches revocation lists over the network
// for that: 0.2 to 0.6 s on the game thread. The check still runs in full; only the
// revocation part is limited to what Windows has cached.

const WinVerifyTrustFn = *const fn (window: ?*anyopaque, action: *anyopaque, data: ?*anyopaque) callconv(.winapi) i32;
var original_win_verify_trust: WinVerifyTrustFn = undefined;

fn calledFromCoherent(return_address: usize) bool {
    var module: ?*anyopaque = null;
    // GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT
    if (GetModuleHandleExW(4 | 2, return_address, &module) == 0 or module == null) return false;
    var path: [260]u16 = undefined;
    const n = GetModuleFileNameW(module, &path, path.len);
    var start: usize = 0;
    for (path[0..n], 0..) |c, i| {
        if (c == '\\' or c == '/') start = i + 1;
    }
    const prefix = std.unicode.utf8ToUtf16LeStringLiteral("coherentui");
    if (n - start < prefix.len) return false;
    for (path[start..][0..prefix.len], prefix) |a, b| {
        if (a > 0x7f or std.ascii.toLower(@intCast(a)) != b) return false;
    }
    return true;
}

fn hookWinVerifyTrust(window: ?*anyopaque, action: *anyopaque, data: ?*anyopaque) callconv(.winapi) i32 {
    if (data) |trust_data| {
        const bytes: [*]u8 = @ptrCast(trust_data);
        const size = std.mem.readInt(u32, bytes[0..4], .little);
        // WINTRUST_DATA.dwProvFlags at 72; WTD_CACHE_ONLY_URL_RETRIEVAL
        if (size >= 76 and calledFromCoherent(@returnAddress())) {
            const flags: *align(1) u32 = @ptrCast(bytes + 72);
            flags.* |= 0x1000;
        }
    }
    return original_win_verify_trust(window, action, data);
}

// ---------------------------------------------------------------------------
// trace: `TEMPEST_FASTSTART_TRACE=<rva hex>` logs the game frames that called that function
// (its first 8 calls) and when. For a function taking up to four integer arguments.

const TracedFn = *const fn (a: usize, b: usize, c: usize, d: usize) callconv(.c) usize;
var original_traced: TracedFn = undefined;
var trace_calls = std.atomic.Value(u32).init(0);

extern "kernel32" fn RtlCaptureStackBackTrace(skip: u32, count: u32, frames: [*]?*anyopaque, hash: ?*u32) callconv(.winapi) u16;

fn hookTraced(a: usize, b: usize, c: usize, d: usize) callconv(.c) usize {
    if (trace_calls.fetchAdd(1, .monotonic) < 8) {
        var frames: [32]?*anyopaque = undefined;
        const n = RtlCaptureStackBackTrace(1, frames.len, &frames, null);
        var text: [32 * 10]u8 = undefined;
        var len: usize = 0;
        for (frames[0..n]) |frame| {
            const address = @intFromPtr(frame);
            if (address < base or address >= base + 0x5000000) continue;
            const written = std.fmt.bufPrint(text[len..], " {x}", .{address - base}) catch break;
            len += written.len;
        }
        log("trace: called from{s}", .{text[0..len]});
    }
    return original_traced(a, b, c, d);
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
        // Applying the audio settings sleeps 0.1 s between two calls that compiled to `ret`:
        // movss xmm0, [0.1] -> xorps xmm0, xmm0; nop
        patchBytes("sleeps: audio settings", rva.audio_settings_sleep, &.{ 0xf3, 0x0f, 0x10, 0x05, 0xc5, 0x9d, 0x35, 0x01 }, &.{ 0x0f, 0x57, 0xc0, 0x0f, 0x1f, 0x44, 0x00, 0x00 });
    }

    // call [rax+0x2a0] (GEngine->GetMaxTickRate), in the frame limiter
    if (enabled("login_frames")) {
        _ = replaceWithCall("login_frames", rva.max_tick_rate_call, &.{ 0xff, 0x90, 0xa0, 0x02, 0x00, 0x00 }, @intFromPtr(&hookMaxTickRate));
    }

    const want_rand = enabled("rand") and
        matches(rva.rand_poll, &.{ 0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x10, 0x48, 0x89, 0x7c, 0x24, 0x18, 0x4c }) and
        matches(rva.rand_add, &.{ 0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0xb8, 0x30, 0x00, 0x00, 0x00, 0xe8 });
    const want_file_size = enabled("file_size") and
        matches(rva.file_size, &.{ 0x40, 0x57, 0x48, 0x83, 0xec, 0x40, 0x45, 0x33, 0xc9, 0x48, 0x8b, 0xca, 0x48, 0xc7, 0x44, 0x24 });
    const want_file_size_by_name = want_file_size and
        matches(rva.file_size_by_name, &.{ 0x41, 0x56, 0x48, 0x83, 0xec, 0x50, 0x48, 0xc7, 0x44, 0x24, 0x20, 0xfe, 0xff, 0xff, 0xff, 0x48, 0x89, 0x5c, 0x24, 0x60 });
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
    original_file_size_by_name = at(FileSize, rva.file_size_by_name);
    original_find_package = at(FindPackage, rva.find_package);
    original_read_compressed = at(ReadCompressed, rva.read_compressed);
    original_crc32 = at(Crc32, rva.crc32);

    for (&chunks) |*chunk| chunk.* = .{ .key = std.mem.zeroes(ChunkKey) };

    detourz.transactionBegin() catch return;
    detourz.updateThread(GetCurrentThread()) catch {};
    const has_rand = want_rand and attach("rand", &original_rand_poll, &hookRandPoll);
    const has_file_size = want_file_size and attach("file_size", &original_file_size, &hookFileSize);
    if (has_file_size and want_file_size_by_name) _ = attach("file_size: by name", &original_file_size_by_name, &hookFileSizeByName);
    const has_packages = want_packages and attach("packages", &original_find_package, &hookFindPackage);
    const has_chunks = want_chunks and attach("chunks", &original_read_compressed, &hookReadCompressed);
    const has_crc = want_crc and attach("crc", &original_crc32, &hookCrc32);
    if (enabled("discovery")) {
        if (matches(rva.http_get, &.{ 0x40, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x48, 0x8d, 0xac, 0x24, 0xf0, 0xfd, 0xff, 0xff }) and
            matches(rva.vector_insert, &.{ 0x48, 0x89, 0x54, 0x24, 0x10, 0x56, 0x57, 0x41, 0x54, 0x41, 0x56, 0x41, 0x57 }))
        {
            original_http_get = at(HttpGet, rva.http_get);
            if (attach("discovery", &original_http_get, &hookHttpGet)) log("discovery: hooked", .{});
        } else {
            log("discovery: unexpected bytes, left alone", .{});
        }
    }
    if (enabled("browser_trust")) {
        if (GetModuleHandleW(std.unicode.utf8ToUtf16LeStringLiteral("wintrust.dll"))) |wintrust| {
            if (GetProcAddress(wintrust, "WinVerifyTrust")) |function| {
                original_win_verify_trust = @ptrCast(function);
                if (attach("browser_trust", &original_win_verify_trust, &hookWinVerifyTrust)) log("browser_trust: hooked", .{});
            }
        } else {
            log("browser_trust: wintrust.dll is not loaded, left alone", .{});
        }
    }
    var trace_text: [16]u8 = undefined;
    const trace_len = GetEnvironmentVariableA("TEMPEST_FASTSTART_TRACE", &trace_text, trace_text.len);
    if (trace_len > 0 and trace_len < trace_text.len) {
        if (std.fmt.parseInt(usize, trace_text[0..trace_len], 16)) |traced| {
            original_traced = at(TracedFn, traced);
            if (attach("trace", &original_traced, &hookTraced)) log("trace: {x} hooked", .{traced});
        } else |_| {}
    }
    detourz.transactionCommit() catch {
        log("faststart: the detours could not be committed", .{});
        return;
    };
    log("faststart: rand={} file_size={} packages={} chunks={} crc={}", .{ has_rand, has_file_size, has_packages, has_chunks, has_crc });

    var seconds: [16]u8 = undefined;
    const seconds_len = GetEnvironmentVariableA("TEMPEST_FASTSTART_PROFILE", &seconds, seconds.len);
    if (seconds_len > 0 and seconds_len < seconds.len) {
        profile_seconds = std.fmt.parseInt(u32, seconds[0..seconds_len], 10) catch 0;
        var which: [16]u8 = undefined;
        const which_len = GetEnvironmentVariableA("TEMPEST_FASTSTART_PROFILE_THREAD", &which, which.len);
        profile_busiest = which_len > 0 and which_len < which.len and std.ascii.eqlIgnoreCase(which[0..which_len], "busiest");
        var window: [16]u8 = undefined;
        const window_len = GetEnvironmentVariableA("TEMPEST_FASTSTART_PROFILE_WINDOW", &window, window.len);
        if (window_len > 0 and window_len < window.len) {
            profile_window_ms = @max(100, std.fmt.parseInt(u32, window[0..window_len], 10) catch profile_window_ms);
        }
        if (profile_seconds != 0) {
            if (CreateThread(null, 0, &profiler, null, 0, null)) |thread| _ = CloseHandle(thread);
        }
    }

    if (enabled("graphics")) {
        // lea ecx, [rdi+0x20]; call Direct3DCreate9 (the import thunk)
        if (matches(rva.display_query_create - 3, &.{ 0x8d, 0x4f, 0x20 })) {
            _ = redirectCall("graphics: display mode query", rva.display_query_create, &.{ 0xe8, 0x45, 0x61, 0xde, 0x00 }, @intFromPtr(&hookCreateD3D9), @ptrCast(&original_create_d3d9));
        }
        if (CreateThread(null, 0, &graphicsWarmer, null, 0, null)) |thread| _ = CloseHandle(thread);
    }

    var has_opens = false;
    if (enabled("opens")) {
        // call [CreateFileW], twice in the async loader's "open this file"
        const in_profile = redirectCall("opens: profile folder", rva.loader_open_in_profile, &.{ 0xff, 0x15, 0xd6, 0x33, 0x2e, 0x02 }, @intFromPtr(&hookOpenInProfile), &open_slots[0]);
        const in_game = redirectCall("opens: game folder", rva.loader_open_in_game, &.{ 0xff, 0x15, 0xec, 0x32, 0x2e, 0x02 }, @intFromPtr(&hookOpenInGame), &open_slots[1]);
        has_opens = in_profile or in_game;
        listing_enabled = in_profile and enabled("listing") and watchesWork();
        log("opens: profile folder listing {s}", .{if (listing_enabled) "on" else "off"});
        if (in_game and enabled("sounds")) {
            if (CreateThread(null, 0, &soundWarmer, null, 0, null)) |thread| _ = CloseHandle(thread);
        }
    }

    if (has_chunks or has_opens) {
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
