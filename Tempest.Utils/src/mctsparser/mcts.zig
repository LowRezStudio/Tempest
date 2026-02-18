const std = @import("std");
const fs = std.fs;
const windows = std.os.windows;

const fnv1_32 = @import("utils.zig").fnv1_32;

pub const FieldType = enum(u8) {
    unknown = 0,
    _num_start,
    byte,
    int,
    word,
    dword,
    float,
    double,
    qword,
    netid,
    datetime,
    _num_end,
    string,
    rowset,
    guid,
    bin,
    _last,
};

pub const NetIdType = enum(u8) {
    unknown = 0,
    local,
    account,
    character,
    clan,
    channel,
    connection,
    instance,
    match,
    player,
    queue,
    server,
    team,
    proxy,
    _max_type,
};

pub const FieldEntry = struct {
    // internal use
    index: usize,

    sort_index: windows.WORD,
    netid_type: NetIdType,
    type: FieldType,
    name: []const u8,

    pub fn init(allocator: std.mem.Allocator, file: fs.File) !struct { entries: []FieldEntry, buffer: []u8 } {
        var buffer: [4096]u8 = undefined;
        var fr = file.reader(&buffer);
        const reader = &fr.interface;

        const file_stat = try file.stat();
        const file_buffer = try reader.readAlloc(allocator, file_stat.size);

        const entries = try loadFromFile(allocator, file_buffer);
        return .{ .entries = entries, .buffer = file_buffer };
    }

    fn loadFromFile(allocator: std.mem.Allocator, file_buffer: []u8) ![]FieldEntry {
        var fields = std.ArrayList(FieldEntry).empty;
        errdefer fields.deinit(allocator);

        var fr = std.Io.Reader.fixed(file_buffer);
        var reader = &fr;

        var index: usize = 0;
        while (true) {
            const sort_index = reader.takeInt(u16, .big) catch |err| switch (err) {
                error.EndOfStream => break,
                else => |e| return e,
            };

            const netid_type = reader.takeInt(u8, .big) catch break;
            const @"type" = reader.takeInt(u8, .big) catch break;
            const name = try reader.takeDelimiter('\x00');

            try fields.append(allocator, .{
                .index = index,
                .sort_index = sort_index,
                .netid_type = @enumFromInt(netid_type),
                .type = @enumFromInt(@"type"),
                .name = name.?,
            });

            index += 1;
        }

        return fields.toOwnedSlice(allocator);
    }
};

pub const FunctionDetail = struct {
    index: windows.DWORD,
    sort_index: windows.WORD,
    flags: windows.WORD,
    name: []const u8,
    wide_name: []const u16,

    pub fn init(allocator: std.mem.Allocator, file: fs.File) !struct { entries: []FunctionDetail, buffer: []u8 } {
        var buffer: [4096]u8 = undefined;
        var fr = file.reader(&buffer);
        const reader = &fr.interface;

        const file_stat = try file.stat();
        const file_buffer = try reader.readAlloc(allocator, file_stat.size);

        const entries = try loadFromFile(allocator, file_buffer);
        return .{ .entries = entries, .buffer = file_buffer };
    }

    fn loadFromFile(allocator: std.mem.Allocator, file_buffer: []u8) ![]FunctionDetail {
        var fields = std.ArrayList(FunctionDetail).empty;
        errdefer fields.deinit(allocator);

        var fr = std.Io.Reader.fixed(file_buffer);
        var reader = &fr;

        while (true) {
            const sort_index = reader.takeInt(u16, .big) catch |err| switch (err) {
                error.EndOfStream => break,
                else => |e| return e,
            };
            const flags = reader.takeInt(u16, .big) catch break;

            const index = reader.takeInt(u32, .big) catch break;
            const name = try reader.takeDelimiter('\x00');

            try fields.append(allocator, .{
                .index = index,
                .sort_index = sort_index,
                .flags = flags,
                .name = name.?,
                .wide_name = &.{},
            });
        }

        return fields.toOwnedSlice(allocator);
    }
};

pub const Fields = struct {
    pub var entries: []FieldEntry = &.{};

    pub var buffer: []const u8 = &.{};
    pub var allocator: ?std.mem.Allocator = null;

    pub fn init(alloc: std.mem.Allocator, file: fs.File) !void {
        if (allocator) |_| return;

        allocator = alloc;

        const results = try FieldEntry.init(allocator.?, file);

        entries = results.entries;
        buffer = results.buffer;
    }

    pub fn deinit() void {
        if (allocator) |alloc| {
            alloc.free(entries);
            alloc.free(buffer);

            entries = &.{};
            buffer = &.{};
            allocator = null;
        }
    }

    pub fn get(index: usize) ?FieldEntry {
        if (index < entries.len) {
            return entries[index];
        }
        return null;
    }

    pub fn format(
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        for (entries) |field| {
            try writer.print("Field: {d} -> {s} ({s} -> {s})\n", .{ field.index, field.name, @tagName(field.type), @tagName(field.netid_type) });
        }
    }
};

pub const Functions = struct {
    pub var entries: std.AutoHashMap(u32, FunctionDetail) = undefined;
    pub var list: []FunctionDetail = &.{};

    pub var buffer: []u8 = &.{};
    pub var allocator: ?std.mem.Allocator = null;

    pub fn init(alloc: std.mem.Allocator, file: fs.File) !void {
        if (allocator) |_| return;

        allocator = alloc;

        const result = try FunctionDetail.init(allocator.?, file);

        var hashmap = std.AutoHashMap(u32, FunctionDetail).init(allocator.?);
        for (result.entries) |function| {
            try hashmap.put(function.index, function);
        }

        entries = hashmap;
        list = result.entries;
        buffer = result.buffer;
    }

    pub fn deinit() void {
        if (allocator) |alloc| {
            entries.deinit();
            alloc.free(list);
            alloc.free(buffer);

            entries = undefined;
            list = &.{};
            buffer = &.{};
            allocator = null;
        }
    }

    pub fn get(hash: u32) ?FunctionDetail {
        if (entries.get(hash)) |value| {
            return value;
        }
        return null;
    }

    pub fn getByName(name: []const u8) ?FunctionDetail {
        const idx = fnv1_32(name);
        return get(idx);
    }

    pub fn format(
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        for (list, 0..) |function, idx| {
            try writer.print("Function: {d} -> {s} ({X})\n", .{ idx, function.name, function.index });
        }
    }
};

pub const CMarshalRow = struct {
    entry_count: windows.DWORD,
    entry_list: ?*CMarshalEntry,
    entry_tail: ?*CMarshalEntry,
    entry_pool: ?*CMarshalEntry,

    pub fn init() CMarshalRow {
        return CMarshalRow{
            .entry_count = 0,
            .entry_list = null,
            .entry_tail = null,
            .entry_pool = null,
        };
    }

    pub fn allocEntry(allocator: std.mem.Allocator, count: i32, head: *?*CMarshalEntry, tail: *?*CMarshalEntry) bool {
        if (count <= 0) {
            std.log.err("AllocEntry count is bad {d}", .{count});
            return false;
        }

        var remaining: i32 = count - 1;
        var first: ?*CMarshalEntry = null;
        var prev: ?*CMarshalEntry = null;

        while (true) {
            const entry = allocator.create(CMarshalEntry) catch return false;
            entry.* = std.mem.zeroes(CMarshalEntry);

            remaining -= 1;

            entry.next = prev;

            if (remaining == -1) {
                head.* = entry;
                tail.* = first;
                return true;
            }

            if (first == null) {
                first = entry;
            }

            prev = entry;
        }
    }

    pub fn freeEntryChain(allocator: std.mem.Allocator, head_entry: ?*CMarshalEntry) bool {
        const pHead = head_entry orelse return true;

        if (pHead.field_id > Fields.entries.len) {
            std.debug.panic("Bad memory detected in entry chain", .{});
            return false;
        }

        var head: *CMarshalEntry = pHead;
        while (true) {
            const m_pNext = head.next;

            allocator.destroy(head);

            const next = m_pNext orelse return true;

            if (pHead.field_id > Fields.entries.len) {
                std.debug.panic("Bad memory detected in entry chain", .{});
                return false;
            }

            head = next;
        }
    }

    pub fn deinit(self: *CMarshalRow, allocator: std.mem.Allocator) void {
        self.clear(allocator);
    }

    pub fn clear(self: *CMarshalRow, allocator: std.mem.Allocator) void {
        if (self.entry_list) |entry_list| {
            if (!freeEntryChain(allocator, entry_list)) {
                std.log.warn("Avoiding freeing a marshal with bad memory.", .{});
                return;
            }
            self.entry_list = null;
        }

        if (self.entry_pool) |entry_pool| {
            if (!freeEntryChain(allocator, entry_pool)) {
                std.log.warn("Avoiding freeing a marshal with bad memory.", .{});
                return;
            }
            self.entry_pool = null;
        }

        self.entry_count = 0;
        self.entry_tail = null;
    }

    pub fn load(self: *CMarshalRow, allocator: std.mem.Allocator, package: *CPackage) bool {
        self.clear(allocator);

        if (package.used == 0) return false;

        var count: u16 = undefined;
        if (package.place + 2 < package.used and package.cur_place + 2 <= 0x7fd) {
            count = std.mem.readInt(u16, package.current.?.net.data[package.cur_place..][0..2], .little);
            package.cur_place += 2;
            package.place += 2;
        } else {
            var count_buf: [2]u8 = undefined;
            _ = package.read(&count_buf, 2);
            count = std.mem.readInt(u16, &count_buf, .little);
        }

        self.entry_count = count;

        if (count == 0xffff) {
            if (package.place + 4 < package.used and package.cur_place + 4 <= 0x7fd) {
                self.entry_count = std.mem.readInt(u32, package.current.?.net.data[package.cur_place..][0..4], .little);
                package.cur_place += 4;
                package.place += 4;
            } else {
                var count_buf: [4]u8 = undefined;
                _ = package.read(&count_buf, 4);
                self.entry_count = std.mem.readInt(u32, &count_buf, .little);
            }
        }

        if (@as(u16, @truncate(self.entry_count)) == 0) return true;

        const alloc_ok = allocEntry(allocator, count, &self.entry_list, &self.entry_tail);
        if (!alloc_ok) return false;

        self.entry_tail = self.entry_list;

        const load_ok = self.loadEntries(allocator, package);

        if (!load_ok) return false;

        self.entry_tail = self.entry_list;
        var next = self.entry_list.?.next;
        var i: u32 = 0;
        while (next != null and i < 0x800) : (i += 1) {
            self.entry_tail = next;
            next = next.?.next;
        }

        return true;
    }

    pub fn loadEntries(self: *CMarshalRow, allocator: std.mem.Allocator, package: *CPackage) bool {
        _ = self;
        _ = allocator;
        _ = package;

        std.log.warn("loadEntries: not implemented", .{});
        return false;
    }
};

pub const CMarshalRowSet = struct {
    rows: []CMarshalRow,
};

pub const CMarshalEntry = struct {
    data: extern union {
        u32_number: u32,
        i32_number: i32,
        f32_number: f32,
        f64_number: f64,
        u64_number: u64,
        wz_local: [4]u16,
        wz_pointer: ?*const u16,
        u8_data: ?*const u8,
        row_set: ?*CMarshalRowSet,
    },
    next: ?*CMarshalEntry,
    field_id: u16,
    size: u16,
};

pub const CMarshal = struct {
    base: CMarshalRow,

    detail: ?*FunctionDetail,
    flags: u8,
    function_id: u32,

    pub fn init(function_id: u32) CMarshal {
        const row = CMarshalRow.init();
        return CMarshal{
            .base = row,
            .detail = null,
            .flags = 0,
            .function_id = function_id,
        };
    }

    pub fn deinit(self: *CMarshal, allocator: std.mem.Allocator) void {
        self.base.deinit(allocator);
    }

    pub fn load(self: *CMarshal, allocator: std.mem.Allocator, package: *CPackage) bool {
        if (package.place + 1 < package.used and package.cur_place + 1 <= 0x7fd) {
            self.flags = package.current.?.net.data[package.cur_place];
            package.cur_place += 1;
            package.place += 1;
        } else {
            var flags_buf: [1]u8 = undefined;
            const read_bytes = package.read(&flags_buf, 1);
            if (read_bytes == 0) return false;

            self.flags = flags_buf[0];
        }

        var function_id: u32 = undefined;
        if (package.place + 4 < package.used and package.cur_place + 4 <= 0x7fd) {
            const cur = package.cur_place;
            function_id = std.mem.readInt(u32, package.current.?.net.data[cur..][0..4], .little);
            package.cur_place += 4;
            package.place += 4;
        } else {
            var func_buf: [4]u8 = undefined;
            const read_bytes = package.read(&func_buf, 4);
            if (read_bytes == 0) return false;

            function_id = std.mem.readInt(u32, &func_buf, .little);
        }

        const func_detail = Functions.get(function_id);

        if (func_detail == null) {
            std.log.warn("Bad marshal, out of range function [{d}]", .{function_id});
            return false;
        }

        const success = self.base.load(allocator, package);

        if (success) {
            self.function_id = function_id;
            return true;
        }

        std.log.warn("Bad marshal [{s}]", .{func_detail.?.name});
        return false;
    }
};

pub const CMpscEntry = struct {
    next: ?*CMpscEntry,
};

pub const CPackPacketNET = struct {
    header: extern union {
        fields: packed struct {
            size: windows.WORD,
            extended: windows.BYTE,
            flags: windows.BYTE,
        },
        all: windows.DWORD,
    },
    data: [0x7fe]windows.BYTE,
};

pub const CPackPacket = struct {
    next: ?*CPackPacket,
    net: CPackPacketNET,
    source_func: windows.DWORD,

    pub fn init(allocator: std.mem.Allocator) !*CPackPacket {
        const packet = try allocator.create(CPackPacket);
        packet.* = .{
            .next = null,
            .net = .{
                .header = .{ .all = 0 },
                .data = std.mem.zeroes([0x7fe]u8),
            },
            .source_func = 0,
        };

        return packet;
    }

    pub fn deinit(self: *CPackPacket, allocator: std.mem.Allocator) void {
        allocator.destroy(self);
    }
};

pub const CPackage = struct {
    base: CMpscEntry,

    packets: ?*CPackPacket,
    used: windows.DWORD,
    place: windows.DWORD,
    current: ?*CPackPacket,
    cur_place: windows.DWORD,
    first_alloc: bool,
    flags: windows.BYTE,
    extended: windows.BYTE,
    p_extended: [0xc]windows.BYTE,
    encoding: windows.BYTE,
    db_action_func: windows.DWORD,

    pub fn init(allocator: std.mem.Allocator) !CPackage {
        const packet = try CPackPacket.init(allocator);

        return CPackage{
            .base = .{ .next = null },
            .packets = packet,
            .used = 0,
            .place = 0,
            .current = packet,
            .cur_place = 0,
            .first_alloc = true,
            .flags = 0,
            .extended = 0,
            .p_extended = std.mem.zeroes([0xc]u8),
            .encoding = 3,
            .db_action_func = 0,
        };
    }

    pub fn deinit(self: *CPackage, allocator: std.mem.Allocator) void {
        freeChain(allocator, self.packets);
        self.packets = null;
        self.current = null;
    }

    pub fn freeChain(allocator: std.mem.Allocator, chain: ?*CPackPacket) void {
        if (chain == null) return;

        var current = chain;
        while (current) |packet| {
            const next = packet.next;
            packet.deinit(allocator);
            current = next;
        }
    }

    pub fn empty(self: *CPackage) void {
        self.used = 0;
        self.place = 0;
        self.cur_place = 0;
        self.first_alloc = true;
        self.current = self.packets;
        self.flags = 0;
        self.extended = 0;
    }

    pub fn setPlace(self: *CPackage, allocator: std.mem.Allocator, place: u32) !u32 {
        self.place = place;
        self.current = self.packets;

        if (place == 0) {
            self.cur_place = 0;
        } else {
            const packet_index = place / 0x7fe;
            self.cur_place = place % 0x7fe;

            var packets_to_skip = packet_index;
            while (packets_to_skip > 0) {
                if (self.current.?.next) |next_packet| {
                    self.current = next_packet;
                    packets_to_skip -= 1;
                } else {
                    const new_packet = try CPackPacket.init(allocator);
                    self.current.?.next = new_packet;
                    self.current = new_packet;
                    packets_to_skip -= 1;
                }
            }
        }

        if (self.place >= self.used) {
            self.used = self.place;
        }

        return self.place;
    }

    pub fn peek(self: *CPackage, buffer: []u8, size: u32) u32 {
        var bytes_to_read = self.used - self.place;
        if (bytes_to_read > size) bytes_to_read = size;

        if (bytes_to_read == 0) return 0;
        if (buffer.len < bytes_to_read) return 0;

        var current = self.current;
        var cur_place = self.cur_place;

        var remaining: u32 = bytes_to_read;
        var buffer_offset: usize = 0;

        while (true) {
            if (cur_place > 0x7fd) {
                current = current.?.next orelse return bytes_to_read - remaining;
                cur_place = 0;
            }

            var chunk = 0x7fe - cur_place;
            if (chunk > remaining) chunk = remaining;

            @memcpy(
                buffer[buffer_offset..][0..chunk],
                current.?.net.data[cur_place..][0..chunk],
            );

            cur_place += chunk;
            buffer_offset += chunk;
            remaining -= chunk;

            if (remaining == 0) break;
        }

        return bytes_to_read;
    }

    pub fn read(self: *CPackage, buffer: []u8, size: u32) u32 {
        var bytes_to_read = self.used - self.place;
        if (bytes_to_read > size) bytes_to_read = size;

        if (bytes_to_read == 0) return 0;
        if (buffer.len < bytes_to_read) return 0;

        var remaining = bytes_to_read;
        var buffer_offset: usize = 0;

        while (true) {
            if (self.cur_place > 0x7fd) {
                const next = self.current.?.next;

                if (next == null) return bytes_to_read - remaining;

                self.current = next;
                self.cur_place = 0;
            }

            var chunk = 0x7fe - self.cur_place;
            if (chunk > remaining) chunk = remaining;

            @memcpy(
                buffer[buffer_offset..][0..chunk],
                self.current.?.net.data[self.cur_place..][0..chunk],
            );

            self.cur_place += chunk;
            self.place += chunk;
            buffer_offset += chunk;
            remaining -= chunk;

            if (remaining == 0) break;
        }

        return bytes_to_read;
    }

    pub fn readValue(self: *CPackage, comptime T: type, value: *T) T {
        const size = @sizeOf(T);

        if (self.place + size < self.used and self.cur_place + size <= 0x7fd) {
            const result = std.mem.readInt(T, self.current.?.net.data[self.cur_place..][0..size], .little);
            value.* = result;
            self.cur_place += size;
            self.place += size;
            return result;
        }

        var buf: [size]u8 = undefined;
        _ = self.read(&buf, size);
        const result = std.mem.readInt(T, &buf, .little);
        value.* = result;
        return result;
    }

    pub fn readBufferSkip(self: *CPackage, size: u32) u32 {
        var bytes_to_skip = self.used - self.place;
        if (bytes_to_skip > size) bytes_to_skip = size;

        if (bytes_to_skip == 0) return 0;

        var remaining: u32 = bytes_to_skip;

        while (true) {
            if (self.cur_place > 0x7fd) {
                const next = self.current.?.next orelse return bytes_to_skip - remaining;
                self.current = next;
                self.cur_place = 0;
            }

            var chunk = 0x7fe - self.cur_place;
            if (chunk > remaining) chunk = remaining;

            self.place += chunk;
            const prev = remaining;
            remaining -= chunk;
            self.cur_place += chunk;

            if (prev == chunk) break;
        }

        return bytes_to_skip;
    }

    pub fn write(self: *CPackage, allocator: std.mem.Allocator, buffer: []const u8, size: u32) !u64 {
        if (size == 0) return 0;
        if (buffer.len < size) return 0;

        var remaining = size;
        var buffer_offset: usize = 0;

        while (remaining > 0) {
            if (self.cur_place > 0x7fd) {
                if (self.current.?.next == null) {
                    const new_packet = try CPackPacket.init(allocator);
                    self.current.?.next = new_packet;
                }

                self.current = self.current.?.next;
                self.cur_place = 0;
            }

            const available_in_packet = 0x7fe - self.cur_place;
            const bytes_to_write = @min(available_in_packet, remaining);

            @memcpy(self.current.?.net.data[self.cur_place..][0..bytes_to_write], buffer[buffer_offset..][0..bytes_to_write]);

            self.cur_place += bytes_to_write;
            self.place += bytes_to_write;
            buffer_offset += bytes_to_write;
            remaining -= bytes_to_write;
        }

        if (self.place > self.used) {
            self.used = self.place;
        }

        return size;
    }

    pub fn readFromFile(self: *CPackage, allocator: std.mem.Allocator, file_path: []const u8, obscure: bool) !u64 {
        self.empty();

        const file = try fs.cwd().openFile(file_path, .{});
        defer file.close();

        const file_stat = try file.stat();
        if (file_stat.size == 0) return 0;

        self.current = self.packets;
        self.used = @intCast(file_stat.size);

        const max_read_size: u64 = @min(file_stat.size, 0x1ff80);
        const max_packet_size = 0x7fe;

        const buffer = try allocator.alloc(u8, max_read_size);
        defer allocator.free(buffer);

        var fr = file.reader(buffer);
        const reader = &fr.interface;

        var total_read: u64 = 0;

        while (total_read < file_stat.size) {
            const read_bytes = try reader.readSliceShort(buffer);
            if (read_bytes == 0) break;

            if (obscure) {
                for (buffer[0..read_bytes]) |*b| {
                    b.* ^= 0x2A;
                }
            }

            total_read += read_bytes;

            var offset: usize = 0;
            while (offset < read_bytes) {
                const bytes_to_copy = @min(read_bytes - offset, max_packet_size);
                @memcpy(self.current.?.net.data[0..bytes_to_copy], buffer[offset..][0..bytes_to_copy]);
                offset += bytes_to_copy;

                const need_more_packets = offset < read_bytes or total_read < file_stat.size;
                if (need_more_packets) {
                    const packet = try CPackPacket.init(allocator);
                    self.current.?.next = packet;
                    self.current = packet;
                }
            }
        }

        return self.used;
    }

    pub fn writeToFile(self: *CPackage, file_path: []const u8, obscure: bool) !u64 {
        const file = try fs.cwd().createFile(file_path, .{});
        defer file.close();

        var buffer: [0x7fe]u8 = undefined;
        var fw = file.writer(&buffer);
        const writer = &fw.interface;

        var total_written: u64 = 0;
        var remaining = self.used;
        var current_packet = self.packets;

        while (current_packet) |packet| {
            const bytes_to_write: usize = @min(remaining, 0x7fe);

            if (obscure) {
                var obscured_buffer: [0x7fe]u8 = undefined;

                for (0..bytes_to_write) |i| {
                    obscured_buffer[i] = packet.net.data[i] ^ 0x2A;
                }

                try writer.writeAll(obscured_buffer[0..bytes_to_write]);
            } else {
                try writer.writeAll(packet.net.data[0..bytes_to_write]);
            }
            try writer.flush();

            total_written += bytes_to_write;
            remaining -= @intCast(bytes_to_write);

            current_packet = packet.next;

            if (current_packet == null) break;
        }

        return total_written;
    }

    pub fn stringReadTruncatedUTF16(
        self: *CPackage,
        allocator: std.mem.Allocator,
        len: u16,
        out_buffer: ?*u16,
        max_len: *u32,
    ) ?*const u16 {
        const dw_bytes: u32 = @intCast(len);

        var result: *u16 = undefined;
        if (out_buffer) |ob| {
            result = ob;
        } else {
            max_len.* = dw_bytes;
            const buf = allocator.alloc(u16, dw_bytes + 1) catch return null;
            result = &buf[0];
        }

        if (dw_bytes < max_len.*) {
            max_len.* = dw_bytes;
        }

        const rcx = max_len.*;

        if (len != 0) {
            const tmp = allocator.alloc(u8, dw_bytes + 1) catch return null;
            defer allocator.free(tmp);

            _ = self.read(tmp[0..dw_bytes], dw_bytes);
            tmp[len] = 0;

            const result_many: [*]u16 = @ptrCast(result);
            for (0..rcx) |i| {
                result_many[i] = @intCast(tmp[i]);
            }
        }

        const result_many: [*]u16 = @ptrCast(result);
        result_many[rcx] = 0;
        return @ptrCast(result);
    }

    pub fn stringReadWide(
        self: *CPackage,
        allocator: std.mem.Allocator,
        avail_len: u16,
        encoding: u8,
        static_buffer: ?[*]u16,
        static_buffer_len: u32,
        buffer: ?*u16,
        final_buffer_len: *u32,
    ) ?*const u16 {
        var p_buffer = buffer;

        if (avail_len != 0) {
            const avail_len_u32: u32 = avail_len;

            if (encoding == 3) {
                const r8: u32 = avail_len;

                if (static_buffer_len > 0 and static_buffer_len >= avail_len_u32) {
                    const sb = static_buffer.?;
                    _ = self.read(@as([*]u8, @ptrCast(sb))[0 .. avail_len_u32 * 4], avail_len_u32 * 4);
                    sb[avail_len] = 0;
                    final_buffer_len.* = avail_len_u32;
                    return @ptrCast(sb);
                } else {
                    if (static_buffer) |sb| {
                        @memset(@as([*]u8, @ptrCast(sb))[0 .. (static_buffer_len * 4) + 4], 0);
                    }

                    if (p_buffer == null) {
                        final_buffer_len.* = avail_len_u32;
                        const buf = allocator.alloc(u16, avail_len_u32 + 1) catch return null;
                        p_buffer = &buf[0];
                    }

                    const cur_len: u16 = @truncate(final_buffer_len.*);

                    if (avail_len > cur_len) {
                        const rbx: u32 = cur_len;
                        const pb_many: [*]u16 = @ptrCast(p_buffer.?);
                        _ = self.read(@as([*]u8, @ptrCast(pb_many))[0 .. rbx * 4], rbx * 4);
                        pb_many[cur_len] = 0;
                        final_buffer_len.* = rbx;
                        // CPackage::ReadBuffer<CNullSpan>(self, (avail_len_u32 - rbx) * 4)
                        return @ptrCast(p_buffer.?);
                    }

                    const pb_many: [*]u16 = @ptrCast(p_buffer.?);
                    _ = self.read(@as([*]u8, @ptrCast(pb_many))[0 .. avail_len_u32 * 4], avail_len_u32 * 4);
                    pb_many[r8] = 0;
                    final_buffer_len.* = avail_len_u32;
                    return @ptrCast(p_buffer.?);
                }
            } else {
                if (encoding == 1) {
                    // CPackage::StringReadWide<UTF8, UTF32>(self, avail_len_u32, static_buffer, static_buffer_len, p_buffer)
                    return null;
                }

                if (encoding == 2) {
                    // CPackage::StringReadWide<UTF16, UTF32>(self, avail_len_u32, static_buffer, static_buffer_len, p_buffer)
                    return null;
                }

                if (encoding == 4) {
                    // CPackage::StringReadWide<UTF32LE, UTF32>(self, avail_len_u32, static_buffer, static_buffer_len, p_buffer)
                    return null;
                }

                if (encoding == 5) {
                    return self.stringReadTruncatedUTF16(allocator, avail_len, buffer, final_buffer_len);
                }

                return null;
            }
        } else {
            if (static_buffer_len == 0 or static_buffer == null) {
                if (final_buffer_len.* != 0 and p_buffer != null) {
                    const pb_many: [*]u16 = @ptrCast(p_buffer.?);
                    pb_many[0] = 0;
                    final_buffer_len.* = 0;
                    return @ptrCast(p_buffer.?);
                }

                final_buffer_len.* = 0;
                const buf = allocator.alloc(u16, 1) catch return null;
                buf[0] = 0;
                return @ptrCast(&buf[0]);
            }

            static_buffer.?[0] = 0;
            final_buffer_len.* = 0;
            return @ptrCast(static_buffer.?);
        }
    }
};
