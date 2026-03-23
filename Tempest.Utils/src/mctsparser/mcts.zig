const std = @import("std");
const fs = std.fs;
const windows = std.os.windows;

const fnv1_32 = @import("utils.zig").fnv1_32;
const rapidjson = @import("hirezrapidjson.zig");

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

        if (self.entry_count == 0) return true;

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
        const count = self.entry_count;
        if (count == 0) return true;

        const max_field_id = Fields.entries.len;
        var processed: i32 = 0;

        const readType = struct {
            fn call(pkg: *CPackage, T: type) T {
                const size = @sizeOf(T);

                if (pkg.place + size < pkg.used and pkg.cur_place + size <= 0x7fd) {
                    const val = std.mem.readInt(T, pkg.current.?.net.data[pkg.cur_place..][0..size], .little);
                    pkg.cur_place += size;
                    pkg.place += size;
                    return val;
                } else {
                    var buf: [size]u8 = undefined;
                    _ = pkg.read(&buf, size);
                    return std.mem.readInt(T, &buf, .little);
                }
            }
        }.call;

        while (processed < count) {
            const raw_header = readType(package, u16);
            const type_tag: u16 = raw_header & 0xf000;
            const type_extra: u16 = raw_header & 0x3f;
            const batch_count: u32 = type_extra;

            if (batch_count == 0) {
                if (type_tag != 0x9000) return false;

                // Clear data
                const tail = self.entry_tail.?;
                tail.data = std.mem.zeroes(@TypeOf(tail.data));

                // Read field id
                const fld = readType(package, u16);
                if (fld > max_field_id) {
                    self.entry_tail.?.field_id = 0;
                    return false;
                }

                const field_entry = Fields.get(fld) orelse {
                    self.entry_tail.?.field_id = 0;
                    return false;
                };

                if (field_entry.type != .string) {
                    self.entry_tail.?.field_id = 0;
                    return false;
                }

                tail.field_id = fld;
                tail.size = 0;
                processed += 1;
                self.entry_tail = tail.next;
                continue;
            }

            switch (type_tag) {
                // Byte values
                0x1000 => {
                    if (type_extra > 0x20) return false;
                    processed += @intCast(batch_count);
                    if (processed > count) return false;

                    var fields_buf: [0x21]u16 = undefined;
                    var values_buf: [0x20]u8 = undefined;

                    const fields_bytes: u32 = batch_count * 2;
                    const values_bytes: u32 = batch_count;

                    if (package.read(std.mem.sliceAsBytes(fields_buf[0..batch_count]), fields_bytes) != fields_bytes) return false;
                    if (package.read(values_buf[0..batch_count], values_bytes) != values_bytes) return false;

                    for (0..batch_count) |i| {
                        const entry = self.entry_tail.?;
                        const fld_id = fields_buf[i];
                        entry.field_id = fld_id;
                        if (fld_id > max_field_id) {
                            entry.field_id = 0;
                            return false;
                        }
                        const fe = Fields.get(fld_id) orelse {
                            entry.field_id = 0;
                            return false;
                        };

                        if (@intFromEnum(fe.type) - 1 > @intFromEnum(FieldType.datetime)) {
                            entry.field_id = 0;
                            return false;
                        }
                        entry.data.u32_number = values_buf[i];
                        self.entry_tail = entry.next;
                    }
                },
                // Word values
                0x2000 => {
                    if (batch_count > 0x20) return false;
                    processed += @intCast(batch_count);
                    if (processed > count) return false;

                    var fields_buf: [0x21]u16 = undefined;
                    var values_buf: [0x20]u16 = undefined;

                    const fields_bytes: u32 = batch_count * 2;
                    const values_bytes: u32 = batch_count * 2;

                    if (package.read(std.mem.sliceAsBytes(fields_buf[0..batch_count]), fields_bytes) != fields_bytes) return false;
                    if (package.read(std.mem.sliceAsBytes(values_buf[0..batch_count]), values_bytes) != values_bytes) return false;

                    for (0..batch_count) |i| {
                        const entry = self.entry_tail.?;
                        const fld_id = fields_buf[i];
                        entry.field_id = fld_id;
                        if (fld_id > max_field_id) {
                            entry.field_id = 0;
                            return false;
                        }
                        const fe = Fields.get(fld_id) orelse {
                            entry.field_id = 0;
                            return false;
                        };
                        if (@intFromEnum(fe.type) - 1 > @intFromEnum(FieldType.datetime)) {
                            entry.field_id = 0;
                            return false;
                        }
                        entry.data.u32_number = values_buf[i];
                        self.entry_tail = entry.next;
                    }
                },
                // Dword values
                0x3000 => {
                    if (batch_count > 0x20) return false;
                    processed += @intCast(batch_count);
                    if (processed > count) return false;

                    var fields_buf: [0x21]u16 = undefined;
                    var values_buf: [0x20]u32 = undefined;

                    const fields_bytes: u32 = batch_count * 2;
                    const values_bytes: u32 = batch_count * 4;

                    if (package.read(std.mem.sliceAsBytes(fields_buf[0..batch_count]), fields_bytes) != fields_bytes) return false;
                    if (package.read(std.mem.sliceAsBytes(values_buf[0..batch_count]), values_bytes) != values_bytes) return false;

                    for (0..batch_count) |i| {
                        const entry = self.entry_tail.?;
                        const fld_id = fields_buf[i];
                        entry.field_id = fld_id;
                        if (fld_id > max_field_id) {
                            entry.field_id = 0;
                            return false;
                        }
                        const fe = Fields.get(fld_id) orelse {
                            entry.field_id = 0;
                            return false;
                        };

                        if (@intFromEnum(fe.type) - 1 > @intFromEnum(FieldType.datetime)) {
                            entry.field_id = 0;
                            return false;
                        }
                        entry.data.u32_number = values_buf[i];
                        self.entry_tail = entry.next;
                    }
                },
                // Qword values
                0x4000 => {
                    if (batch_count > 0x20) return false;
                    processed += @intCast(batch_count);
                    if (processed > count) return false;

                    var fields_buf: [0x21]u16 = undefined;
                    var values_buf: [0x20]u64 = undefined;

                    const fields_bytes: u32 = batch_count * 2;
                    const values_bytes: u32 = batch_count * 8;

                    if (package.read(std.mem.sliceAsBytes(fields_buf[0..batch_count]), fields_bytes) != fields_bytes) return false;
                    if (package.read(std.mem.sliceAsBytes(values_buf[0..batch_count]), values_bytes) != values_bytes) return false;

                    for (0..batch_count) |i| {
                        const entry = self.entry_tail.?;
                        const fld_id = fields_buf[i];
                        entry.field_id = fld_id;
                        if (fld_id > max_field_id) {
                            entry.field_id = 0;
                            return false;
                        }
                        const fe = Fields.get(fld_id) orelse {
                            entry.field_id = 0;
                            return false;
                        };

                        if (@intFromEnum(fe.type) - 1 > @intFromEnum(FieldType.datetime)) {
                            entry.field_id = 0;
                            return false;
                        }
                        entry.data.u64_number = values_buf[i];
                        self.entry_tail = entry.next;
                    }
                },
                // String
                0x5000 => {
                    const fld = readType(package, u16);
                    const entry = self.entry_tail.?;
                    entry.field_id = fld;

                    if (fld > max_field_id) {
                        entry.field_id = 0;
                        return false;
                    }
                    const fe = Fields.get(fld) orelse {
                        entry.field_id = 0;
                        return false;
                    };
                    if (@intFromEnum(fe.type) != 0xc) {
                        entry.field_id = 0;
                        return false;
                    }

                    if (package.used - package.place <= 1) {
                        entry.field_id = 0;
                        return false;
                    }

                    const raw_avail = readType(package, u16);

                    const encoding: u8 = if (@as(i16, @bitCast(raw_avail)) < 0) 4 else 5;
                    const avail_len: u16 = if (encoding == 4) raw_avail & 0x7fff else raw_avail;

                    if (@as(u32, avail_len) > package.used - package.place) {
                        entry.field_id = 0;
                        return false;
                    }

                    var final_len: u32 = 0;
                    const str_ptr = package.stringReadWide(
                        allocator,
                        avail_len,
                        encoding,
                        null,
                        0,
                        null,
                        &final_len,
                    );

                    entry.data.wz_pointer = str_ptr;

                    entry.size = @truncate(final_len);
                    processed += @intCast(batch_count);
                    self.entry_tail = entry.next;
                },
                // String again
                0xA000 => {
                    if (batch_count - 1 > 4) {
                        self.entry_tail.?.field_id = 0;
                        return false;
                    }

                    const fld = readType(package, u16);

                    const entry = self.entry_tail.?;
                    entry.field_id = fld;

                    if (fld > max_field_id) {
                        entry.field_id = 0;
                        return false;
                    }
                    const fe = Fields.get(fld) orelse {
                        entry.field_id = 0;
                        return false;
                    };
                    if (@intFromEnum(fe.type) != 0xc) {
                        entry.field_id = 0;
                        return false;
                    }

                    if (package.used - package.place <= 1) {
                        entry.field_id = 0;
                        return false;
                    }

                    const avail_len = readType(package, u16);
                    if (avail_len > package.used - package.place) {
                        entry.field_id = 0;
                        return false;
                    }

                    var chr: u32 = 0;
                    const str_ptr = package.stringReadWide(
                        allocator,
                        avail_len,
                        @intCast(batch_count),
                        @ptrCast(&entry.data.u32_number),
                        3,
                        null,
                        &chr,
                    );

                    if (str_ptr == null) {
                        entry.field_id = 0;
                        return false;
                    }

                    const data_addr: usize = @intFromPtr(&entry.data);
                    const str_addr: usize = @intFromPtr(str_ptr.?);
                    if (str_addr != data_addr) {
                        entry.data.wz_pointer = str_ptr;
                    }

                    processed += @intCast(batch_count);
                    entry.size = @truncate(chr);
                    self.entry_tail = entry.next;

                    std.debug.print("Entry field: {s}\n", .{Fields.get(entry.field_id).?.name});
                    std.debug.print("Entry size: {d}\n", .{entry.size});
                    std.debug.print("Entry data: ", .{});

                    if (str_ptr) |sp| {
                        var buf: [256]u8 = undefined;
                        var oi: usize = 0;
                        var ii: usize = 0;
                        while (ii < 64 and sp[ii] != 0 and oi < buf.len - 4) : (ii += 1) {
                            const c = sp[ii];
                            if (c <= 0x7F) {
                                buf[oi] = @as(u8, @truncate(c));
                                oi += 1;
                            } else if (c <= 0x7FF) {
                                buf[oi + 0] = @as(u8, @truncate(0xC0 | (c >> 6)));
                                buf[oi + 1] = @as(u8, @truncate(0x80 | (c & 0x3F)));
                                oi += 2;
                            } else if (c <= 0xFFFF) {
                                buf[oi + 0] = @as(u8, @truncate(0xE0 | (c >> 12)));
                                buf[oi + 1] = @as(u8, @truncate(0x80 | ((c >> 6) & 0x3F)));
                                buf[oi + 2] = @as(u8, @truncate(0x80 | (c & 0x3F)));
                                oi += 3;
                            } else if (c <= 0x10FFFF) {
                                buf[oi + 0] = @as(u8, @truncate(0xF0 | (c >> 18)));
                                buf[oi + 1] = @as(u8, @truncate(0x80 | ((c >> 12) & 0x3F)));
                                buf[oi + 2] = @as(u8, @truncate(0x80 | ((c >> 6) & 0x3F)));
                                buf[oi + 3] = @as(u8, @truncate(0x80 | (c & 0x3F)));
                                oi += 4;
                            } else {
                                buf[oi + 0] = 0xEF;
                                buf[oi + 1] = 0xBF;
                                buf[oi + 2] = 0xFD;
                                oi += 3;
                            }
                        }
                        std.log.warn("DBG[{d}]: {s}", .{ entry.field_id, buf[0..oi] });
                    }
                },
                else => {
                    std.log.warn("Unknown type_tag: {x}", .{type_tag});
                    return false;
                },
            }

            //     0x6000 => {
            //         // Rowset
            //         const fld = readU16(package);
            //         const entry = self.entry_tail.?;
            //         entry.field_id = fld;
            //
            //         if (fld > max_field_id) {
            //             entry.field_id = 0;
            //             return false;
            //         }
            //         const fe = Fields.get(fld) orelse {
            //             entry.field_id = 0;
            //             return false;
            //         };
            //         if (@intFromEnum(fe.type) != 0xd) {
            //             entry.field_id = 0;
            //             return false;
            //         }
            //
            //         if (entry.data.row_set == null) {
            //             // TODO: CMarshalRowSet::CMarshalRowSet() + allocate
            //             // entry.data.row_set = try allocator.create(CMarshalRowSet)
            //             std.log.warn("loadEntries: 0x6000 CMarshalRowSet alloc not implemented", .{});
            //             return false;
            //         }
            //
            //         // TODO: CMarshalRowSet::Load(entry.data.row_set, package)
            //         std.log.warn("loadEntries: 0x6000 CMarshalRowSet::Load not implemented", .{});
            //         return false;
            //     },
            //
            //     0x7000 => {
            //         // GUID
            //         const fld = readU16(package);
            //         const entry = self.entry_tail.?;
            //         entry.field_id = fld;
            //
            //         if (fld > max_field_id) {
            //             entry.field_id = 0;
            //             return false;
            //         }
            //         const fe = Fields.get(fld) orelse {
            //             entry.field_id = 0;
            //             return false;
            //         };
            //         if (@intFromEnum(fe.type) != 0xe) {
            //             entry.field_id = 0;
            //             return false;
            //         }
            //
            //         if (entry.data.u8_data == null) {
            //             entry.size = 0x10;
            //             const guid_buf = allocator.alloc(u8, 0x10) catch {
            //                 entry.field_id = 0;
            //                 return false;
            //             };
            //             entry.data.u8_data = @ptrCast(guid_buf.ptr);
            //         }
            //
            //         processed += @intCast(batch_count);
            //         // TODO: CTgGuid::Load(entry.data.u8_data, package)
            //         std.log.warn("loadEntries: 0x7000 CTgGuid::Load not implemented", .{});
            //         self.entry_tail = entry.next;
            //     },
            //
            //     0x8000 => {
            //         // Binary blob
            //         const fld = readU16(package);
            //         const entry = self.entry_tail.?;
            //         entry.field_id = fld;
            //
            //         if (fld > max_field_id) {
            //             entry.field_id = 0;
            //             return false;
            //         }
            //         const fe = Fields.get(fld) orelse {
            //             entry.field_id = 0;
            //             return false;
            //         };
            //         if (@intFromEnum(fe.type) != 0xf) {
            //             entry.field_id = 0;
            //             return false;
            //         }
            //
            //         // Free existing buffer if present
            //         if (entry.data.u8_data != null and entry.size != 0) {
            //             // TODO: operator delete[] — need to track allocation properly
            //             // allocator.free(entry.data.u8_data[0..entry.size])
            //             entry.data.u8_data = null;
            //         }
            //
            //         var size_buf: [2]u8 = undefined;
            //         if (package.read(&size_buf, 2) != 2) {
            //             entry.size = 0;
            //             entry.field_id = 0;
            //             return false;
            //         }
            //         const sz = std.mem.readInt(u16, &size_buf, .little);
            //         entry.size = sz;
            //         self.entry_tail = entry.next;
            //
            //         if (sz > 0) {
            //             const buf = allocator.alloc(u8, sz) catch {
            //                 entry.field_id = 0;
            //                 return false;
            //             };
            //             entry.data.u8_data = @ptrCast(buf.ptr);
            //             if (package.read(buf, sz) != sz) {
            //                 allocator.free(buf);
            //                 entry.size = 0;
            //                 entry.field_id = 0;
            //                 return false;
            //             }
            //         }
            //
            //         processed += @intCast(batch_count);
            //     },
            //
            //     0x9000 => {
            //         // NetId — 1 to 3 u16 components
            //         if (header_param > 3) {
            //             self.entry_tail.?.field_id = 0;
            //             return false;
            //         }
            //
            //         const entry = self.entry_tail.?;
            //         entry.data = std.mem.zeroes(@TypeOf(entry.data));
            //
            //         const fld = readU16(package);
            //         if (fld > max_field_id) {
            //             entry.field_id = 0;
            //             return false;
            //         }
            //         const fe = Fields.get(fld) orelse {
            //             entry.field_id = 0;
            //             return false;
            //         };
            //         if (@intFromEnum(fe.type) != 0xc) {
            //             entry.field_id = 0;
            //             return false;
            //         }
            //
            //         // Read 1, 2, or 3 u16 components into wz_local
            //         if (batch_count >= 1) {
            //             entry.data.wz_local[0] = readU16(package);
            //         }
            //         if (batch_count >= 2) {
            //             entry.data.wz_local[1] = readU16(package);
            //         }
            //         if (batch_count == 3) {
            //             entry.data.wz_local[2] = readU16(package);
            //         }
            //
            //         entry.field_id = fld;
            //         entry.size = @intCast(batch_count);
            //         processed += 1;
            //         self.entry_tail = entry.next;
            //     },
        }

        return true;
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
        wz_pointer: ?[*:0]const u32,
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
        wlen: u16,
        pw_buffer: ?[*]u32,
        max_len: *u32,
    ) ?[*:0]const u32 {
        const dw_bytes: u32 = wlen;

        const result: [*]u32 = if (pw_buffer) |buf| blk: {
            break :blk buf;
        } else blk: {
            max_len.* = dw_bytes;
            const new_buf = allocator.alloc(u32, dw_bytes + 1) catch return null;
            break :blk new_buf.ptr;
        };

        if (dw_bytes < max_len.*) {
            max_len.* = dw_bytes;
        }

        if (wlen == 0) {
            result[max_len.*] = 0;
            return @as([*:0]u32, @ptrCast(result));
        }

        const tmp = allocator.alloc(u8, dw_bytes + 1) catch return null;
        defer allocator.free(tmp);
        tmp[dw_bytes] = 0;

        _ = self.read(tmp[0..dw_bytes], dw_bytes);

        const count = max_len.*;
        for (0..count) |i| {
            result[i] = tmp[i];
        }

        result[count] = 0;
        return @as([*:0]u32, @ptrCast(result));
    }

    // New function: UTF-8 to UTF-32 decoding using HirezRapidJson
    pub fn stringReadWideUtf8ToUtf32(
        self: *CPackage,
        allocator: std.mem.Allocator,
        w_available_len: u16,
        wz_static_buffer: ?[*]u32,
        static_buffer_len: u32,
        wz_buffer: ?[*]u32,
        final_buffer_len: *u32,
    ) ?[*:0]const u32 {
        var pwz_buffer = wz_buffer;

        // Save position for potential restore after static-buffer probing
        const saved_place = self.place;
        const saved_current = self.current;
        const saved_cur_place = self.cur_place;

        // === Path 1: Static buffer provided and has sufficient capacity ===
        // static_buffer_len is in codepoints (UTF-32), each 4 bytes
        if (static_buffer_len > 0 and static_buffer_len >= w_available_len) {
            var output_idx: u32 = 0;
            var stream = rapidjson.CPackageStringStream{
                .package = self,
                .available = w_available_len,
                .read = 0,
                .underflowed = false,
            };

            while (output_idx < w_available_len and !stream.underflowed) {
                var codepoint: u32 = 0;
                // Decode returns true on success OR on clean underflow; check underflowed flag
                if (!rapidjson.HirezRapidJson.decode(&stream, &codepoint)) {
                    break;
                }
                // Stop on null terminator (matches assembly's !c check)
                if (codepoint == 0) break;

                // Check if static buffer would overflow
                if (output_idx >= static_buffer_len) {
                    // Restore package position and exit
                    self.place = saved_place;
                    self.current = saved_current;
                    self.cur_place = saved_cur_place;
                    break;
                }

                wz_static_buffer.?[output_idx] = codepoint;
                output_idx += 1;
            }

            // Null-terminate and return static buffer
            wz_static_buffer.?[output_idx] = 0;
            final_buffer_len.* = output_idx;
            return @as([*:0]const u32, @ptrCast(wz_static_buffer.?));
        }

        // === Zero out static buffer if provided (safety) ===
        if (wz_static_buffer) |static_buf| {
            const zero_count = static_buffer_len + 1;
            @memset(static_buf[0..zero_count], 0);
        }

        // === Allocate dynamic output buffer if not provided ===
        if (pwz_buffer == null) {
            final_buffer_len.* = w_available_len; // Worst case: 1 byte -> 1 codepoint
            const new_buf = allocator.alloc(u32, @as(usize, w_available_len) + 1) catch return null;
            pwz_buffer = new_buf.ptr;
        }

        // === Restore position before main decoding pass ===
        self.place = saved_place;
        self.current = saved_current;
        self.cur_place = saved_cur_place;

        const output_capacity = final_buffer_len.*;

        // === Path 2: Output buffer has enough capacity for worst-case ===
        if (output_capacity >= w_available_len) {
            var output_idx: u32 = 0;
            var stream = rapidjson.CPackageStringStream{
                .package = self,
                .available = w_available_len,
                .read = 0,
                .underflowed = false,
            };

            while (output_idx < w_available_len and !stream.underflowed) {
                var codepoint: u32 = 0;
                if (!rapidjson.HirezRapidJson.decode(&stream, &codepoint)) {
                    break;
                }
                if (codepoint == 0) break;

                pwz_buffer.?[output_idx] = codepoint;
                output_idx += 1;
            }

            // Skip any unconsumed input bytes
            if (stream.read < w_available_len) {
                _ = self.readBufferSkip(w_available_len - stream.read);
            }

            pwz_buffer.?[output_idx] = 0;
            final_buffer_len.* = output_idx;
            return @as([*:0]const u32, @ptrCast(pwz_buffer.?));
        }

        // === Path 3: Output buffer is smaller - decode on-the-fly, skip excess ===
        var output_idx: u32 = 0;
        var stream = rapidjson.CPackageStringStream{
            .package = self,
            .available = w_available_len,
            .read = 0,
            .underflowed = false,
        };

        while (!stream.underflowed and stream.read < w_available_len) {
            var codepoint: u32 = 0;
            if (!rapidjson.HirezRapidJson.decode(&stream, &codepoint)) {
                break;
            }
            if (codepoint == 0) break;

            // Only write if buffer has space
            if (output_idx < output_capacity) {
                pwz_buffer.?[output_idx] = codepoint;
                output_idx += 1;
            }
            // Continue consuming input even if output buffer is full (to advance package position)
        }

        // Skip any remaining unconsumed bytes
        if (stream.read < w_available_len) {
            _ = self.readBufferSkip(w_available_len - stream.read);
        }

        pwz_buffer.?[output_idx] = 0;
        final_buffer_len.* = output_idx;
        return @as([*:0]const u32, @ptrCast(pwz_buffer.?));
    }

    // Helper: Check if code unit is high surrogate [0xD800, 0xDBFF]
    fn isHighSurrogate(cu: u32) bool {
        return cu >= 0xD800 and cu <= 0xDBFF;
    }

    // Helper: Check if code unit is low surrogate [0xDC00, 0xDFFF]
    fn isLowSurrogate(cu: u32) bool {
        return cu >= 0xDC00 and cu <= 0xDFFF;
    }

    // Helper: Combine surrogate pair into UTF-32 codepoint
    fn combineSurrogates(high: u32, low: u32) u32 {
        return 0x10000 + (((high & 0x3FF) << 10) | (low & 0x3FF));
    }

    // Helper: Read a single 4-byte UTF-16 unit from package
    fn readUtf16Unit4Byte(self: *CPackage, buf: *[4]u8) ?u32 {
        if (self.read(buf, 4) != 4) return null;
        return std.mem.readInt(u32, buf, .little);
    }

    // Helper: Peek a single 4-byte UTF-16 unit from package (does not advance)
    fn peekUtf16Unit4Byte(self: *CPackage, buf: *[4]u8) ?u32 {
        if (self.peek(buf, 4) != 4) return null;
        return std.mem.readInt(u32, buf, .little);
    }

    pub fn stringReadWideUtf16WideToUtf32(
        self: *CPackage,
        allocator: std.mem.Allocator,
        w_available_len: u16,
        wz_static_buffer: ?[*]u32,
        static_buffer_len: u32,
        wz_buffer: ?[*]u32,
        final_buffer_len: *u32,
    ) ?[*:0]const u32 {
        var pwz_buffer = wz_buffer;

        // Save position for potential restore after static-buffer probing
        const saved_place = self.place;
        const saved_current = self.current;
        const saved_cur_place = self.cur_place;

        // === Path 1: Static buffer provided and has sufficient capacity ===
        // Assembly check: (static_buffer_len << 2) >= w_available_len
        if (static_buffer_len > 0 and (static_buffer_len << 2) >= w_available_len) {
            var input_idx: u32 = 0;
            var output_idx: u32 = 0;
            var peek_buf: [4]u8 = undefined;
            var read_buf: [4]u8 = undefined;

            while (input_idx < w_available_len) {
                // Peek to check for null terminator or validity
                const cu_peek = self.peekUtf16Unit4Byte(&peek_buf) orelse break;
                if (cu_peek == 0) break; // null terminator

                // Consume the unit
                _ = self.readUtf16Unit4Byte(&read_buf) orelse break;
                const cu = cu_peek; // Use peeked value to ensure consistency

                var codepoint = cu;
                var consumed_units: u32 = 1;

                // Handle UTF-16 surrogate pairs
                if (isHighSurrogate(cu)) {
                    // Need low surrogate next
                    if (input_idx + 1 >= w_available_len) break;

                    const low_peek = self.peekUtf16Unit4Byte(&peek_buf) orelse break;
                    if (!isLowSurrogate(low_peek)) break; // invalid pair

                    _ = self.readUtf16Unit4Byte(&read_buf) orelse break;
                    codepoint = combineSurrogates(cu, low_peek);
                    consumed_units = 2;
                } else if (cu >= 0xD800 and cu <= 0xDFFF) {
                    // Lone low surrogate or invalid range - stop decoding
                    break;
                }

                // Check if static buffer would overflow
                if (output_idx >= static_buffer_len) {
                    // Restore package position and exit (matching assembly behavior)
                    self.place = saved_place;
                    self.current = saved_current;
                    self.cur_place = saved_cur_place;
                    break;
                }

                wz_static_buffer.?[output_idx] = codepoint;
                output_idx += 1;
                input_idx += consumed_units;
            }

            // If we hit null terminator successfully within static buffer limits
            if (input_idx < w_available_len or (output_idx < static_buffer_len and peek_buf[0] == 0 and peek_buf[1] == 0 and peek_buf[2] == 0 and peek_buf[3] == 0)) {
                // Null-terminate and return static buffer
                wz_static_buffer.?[output_idx] = 0;
                final_buffer_len.* = output_idx;
                return @as([*:0]const u32, @ptrCast(wz_static_buffer.?));
            }

            // If static buffer filled up or loop broke early, fall through to dynamic path
            // Restore position first
            self.place = saved_place;
            self.current = saved_current;
            self.cur_place = saved_cur_place;
        }

        // === Zero out static buffer if provided (safety, matching assembly) ===
        if (wz_static_buffer) |static_buf| {
            const zero_count = static_buffer_len + 1;
            @memset(@as([*]u8, @ptrCast(static_buf))[0 .. zero_count * 4], 0);
        }

        // === Allocate dynamic output buffer if not provided ===
        if (pwz_buffer == null) {
            final_buffer_len.* = w_available_len; // worst case: all BMP chars, 1:1 mapping
            const new_buf = allocator.alloc(u32, @as(usize, w_available_len) + 1) catch return null;
            pwz_buffer = new_buf.ptr;
        }

        const output_capacity = final_buffer_len.*;
        var input_idx: u32 = 0;
        var output_idx: u32 = 0;
        var buffer_full = false;
        var read_buf: [4]u8 = undefined;

        // === Path 2 & 3: Dynamic Buffer (Unified Loop with Overflow Handling) ===
        // Assembly distinguishes based on output_capacity >= w_available_len
        // We can handle both in one loop with a flag for skipping

        const is_large_buffer = output_capacity >= w_available_len;

        if (is_large_buffer) {
            // Path 2: Bulk read first (optimization matching assembly)
            while (input_idx < w_available_len) {
                const cu = self.readUtf16Unit4Byte(&read_buf) orelse break;
                pwz_buffer.?[input_idx] = cu;
                if (cu == 0) break;
                input_idx += 1;
            }
            const raw_count = input_idx;

            // Compact surrogates in-place
            var read_idx: u32 = 0;
            var write_idx: u32 = 0;

            while (read_idx < raw_count) {
                const cu = pwz_buffer.?[read_idx];
                if (cu == 0) break;

                var codepoint = cu;
                var consumed: u32 = 1;

                if (isHighSurrogate(cu)) {
                    if (read_idx + 1 < raw_count) {
                        const low = pwz_buffer.?[read_idx + 1];
                        if (isLowSurrogate(low)) {
                            codepoint = combineSurrogates(cu, low);
                            consumed = 2;
                        }
                    }
                } else if (cu >= 0xD800 and cu <= 0xDFFF) {
                    break; // Invalid lone surrogate
                }

                if (write_idx < output_capacity) {
                    pwz_buffer.?[write_idx] = codepoint;
                    write_idx += 1;
                } else {
                    buffer_full = true;
                }

                read_idx += consumed;
            }
            output_idx = write_idx;
            input_idx = read_idx;
        } else {
            // Path 3: Small buffer - decode on-the-fly
            while (input_idx < w_available_len) {
                const cu = self.readUtf16Unit4Byte(&read_buf) orelse break;
                if (cu == 0) break;

                var codepoint = cu;
                var consumed_units: u32 = 1;

                if (isHighSurrogate(cu)) {
                    if (input_idx + 1 >= w_available_len) break;
                    const low = self.readUtf16Unit4Byte(&read_buf) orelse break;
                    if (!isLowSurrogate(low)) break;
                    codepoint = combineSurrogates(cu, low);
                    consumed_units = 2;
                } else if (cu >= 0xD800 and cu <= 0xDFFF) {
                    break;
                }

                if (output_idx < output_capacity) {
                    pwz_buffer.?[output_idx] = codepoint;
                    output_idx += 1;
                } else {
                    buffer_full = true;
                }

                input_idx += consumed_units;
            }
        }

        // Skip any remaining unconsumed input bytes (units * 4)
        if (input_idx < w_available_len) {
            const skip_bytes = (w_available_len - input_idx) * 4;
            _ = self.readBufferSkip(skip_bytes);
        }

        pwz_buffer.?[output_idx] = 0;
        final_buffer_len.* = output_idx;
        return @as([*:0]const u32, @ptrCast(pwz_buffer.?));
    }

    // === Helper methods for UTF-16 (2-byte) decoding ===

    // Helper: Check if code unit is high surrogate [0xD800, 0xDBFF]
    fn isHighSurrogate16(cu: u16) bool {
        return cu >= 0xD800 and cu <= 0xDBFF;
    }

    // Helper: Check if code unit is low surrogate [0xDC00, 0xDFFF]
    fn isLowSurrogate16(cu: u16) bool {
        return cu >= 0xDC00 and cu <= 0xDFFF;
    }

    // Helper: Combine surrogate pair into UTF-32 codepoint
    fn combineSurrogates16(high: u16, low: u16) u32 {
        return 0x10000 + (((@as(u32, high) & 0x3FF) << 10) | (@as(u32, low) & 0x3FF));
    }

    // Helper: Read a single 2-byte UTF-16 unit from package
    fn readUtf16Unit2Byte(self: *CPackage) ?u16 {
        if (self.place + 2 > self.used or self.cur_place + 2 > 0x7fd) {
            var buf: [2]u8 = undefined;
            if (self.read(&buf, 2) != 2) return null;
            return std.mem.readInt(u16, &buf, .little);
        } else {
            const val = std.mem.readInt(u16, self.current.?.net.data[self.cur_place..][0..2], .little);
            self.cur_place += 2;
            self.place += 2;
            return val;
        }
    }

    // Helper: Peek a single 2-byte UTF-16 unit from package (does not advance)
    fn peekUtf16Unit2Byte(self: *CPackage) ?u16 {
        var current = self.current;
        var cur_place = self.cur_place;

        if (cur_place > 0x7fd) {
            current = current.?.next orelse return null;
            cur_place = 0;
        }

        if (self.place + 2 > self.used) return null;

        return std.mem.readInt(u16, current.?.net.data[cur_place..][0..2], .little);
    }

    // === UTF-16 (2-byte) to UTF-32 decoding ===
    pub fn stringReadWideUtf16ToUtf32(
        self: *CPackage,
        allocator: std.mem.Allocator,
        w_available_len: u16,
        wz_static_buffer: ?[*]u32,
        static_buffer_len: u32,
        wz_buffer: ?[*]u32,
        final_buffer_len: *u32,
    ) ?[*:0]const u32 {
        var pwz_buffer = wz_buffer;

        // Save position for potential restore after static-buffer probing
        const saved_place = self.place;
        const saved_current = self.current;
        const saved_cur_place = self.cur_place;

        // === Path 1: Static buffer provided and has sufficient capacity ===
        // Assembly check: (static_buffer_len << 2) >= w_available_len
        // static_buffer_len is in UTF-32 codepoints (4 bytes each)
        // w_available_len is in UTF-16 code units (2 bytes each)
        if (static_buffer_len > 0 and (static_buffer_len << 2) >= w_available_len) {
            var input_idx: u32 = 0;
            var output_idx: u32 = 0;

            while (input_idx < w_available_len) {
                // Peek to check for null terminator
                const cu_peek = self.peekUtf16Unit2Byte() orelse break;
                if (cu_peek == 0) break; // null terminator

                // Consume the unit
                const cu = self.readUtf16Unit2Byte() orelse break;

                var codepoint: u32 = cu;
                var consumed_units: u32 = 1;

                // Handle UTF-16 surrogate pairs
                // Assembly check: (cu + 0x2800 > 0x7ff) means NOT in surrogate range
                // Equivalent to: cu < 0xD800 or cu > 0xDFFF
                if (cu >= 0xD800 and cu <= 0xDFFF) {
                    // In surrogate range - check if high surrogate
                    if (cu > 0xDBFF) {
                        // Low surrogate without high - invalid
                        break;
                    }

                    // High surrogate - need low surrogate next
                    if (input_idx + 1 >= w_available_len) break;

                    const low_peek = self.peekUtf16Unit2Byte() orelse break;
                    // Assembly check: (low + 0x2400 <= 0x3ff) means low in [0xDC00, 0xDFFF]
                    if (low_peek < 0xDC00 or low_peek > 0xDFFF) break;

                    const low = self.readUtf16Unit2Byte() orelse break;
                    codepoint = combineSurrogates16(cu, low);
                    consumed_units = 2;
                }

                // Check if static buffer would overflow
                if (output_idx >= static_buffer_len) {
                    // Restore package position and exit
                    self.place = saved_place;
                    self.current = saved_current;
                    self.cur_place = saved_cur_place;
                    break;
                }

                wz_static_buffer.?[output_idx] = codepoint;
                output_idx += 1;
                input_idx += consumed_units;
            }

            // Null-terminate and return static buffer
            wz_static_buffer.?[output_idx] = 0;
            final_buffer_len.* = output_idx;
            return @as([*:0]const u32, @ptrCast(wz_static_buffer.?));
        }

        // === Zero out static buffer if provided (safety) ===
        if (wz_static_buffer) |static_buf| {
            const zero_count = static_buffer_len + 1;
            @memset(@as([*]u8, @ptrCast(static_buf))[0 .. zero_count * 4], 0);
        }

        // === Allocate dynamic output buffer if not provided ===
        if (pwz_buffer == null) {
            final_buffer_len.* = w_available_len; // worst case: all BMP chars
            const new_buf = allocator.alloc(u32, @as(usize, w_available_len) + 1) catch return null;
            pwz_buffer = new_buf.ptr;
        }

        const output_capacity = final_buffer_len.*;
        const is_large_buffer = output_capacity >= w_available_len;

        // === Path 2: Large buffer - bulk read then compact ===
        if (is_large_buffer) {
            var input_idx: u32 = 0;

            // First pass: bulk-copy UTF-16 code units to output buffer
            while (input_idx < w_available_len) {
                const cu = self.readUtf16Unit2Byte() orelse break;
                pwz_buffer.?[input_idx] = cu;
                if (cu == 0) break;
                input_idx += 1;
            }
            const raw_count = input_idx;

            // Second pass: in-place surrogate pair compaction
            var read_idx: u32 = 0;
            var write_idx: u32 = 0;
            var buffer_overflow = false;
            var consumed_input: u32 = 0;

            while (read_idx < raw_count) {
                const cu = pwz_buffer.?[read_idx];
                if (cu == 0) break;

                var codepoint: u32 = cu;
                var consumed: u32 = 1;

                if (cu >= 0xD800 and cu <= 0xDFFF) {
                    if (cu > 0xDBFF) {
                        // Invalid low surrogate
                        break;
                    }
                    if (read_idx + 1 < raw_count) {
                        const low = pwz_buffer.?[read_idx + 1];
                        if (low >= 0xDC00 and low <= 0xDFFF) {
                            codepoint = combineSurrogates16(@intCast(cu), @intCast(low));
                            consumed = 2;
                        }
                    }
                }

                if (write_idx < output_capacity) {
                    pwz_buffer.?[write_idx] = codepoint;
                    write_idx += 1;
                } else {
                    buffer_overflow = true;
                }

                read_idx += consumed;
                consumed_input += consumed;
            }

            // Skip any unconsumed input bytes
            if (consumed_input < w_available_len) {
                const skip_bytes = (w_available_len - consumed_input) * 2;
                _ = self.readBufferSkip(skip_bytes);
            }

            pwz_buffer.?[write_idx] = 0;
            final_buffer_len.* = write_idx;
            return @as([*:0]const u32, @ptrCast(pwz_buffer.?));
        }

        // === Path 3: Small buffer - decode on-the-fly ===
        var input_idx: u32 = 0;
        var output_idx: u32 = 0;
        var buffer_full = false;

        while (input_idx < w_available_len) {
            const cu = self.readUtf16Unit2Byte() orelse break;
            if (cu == 0) break;

            var codepoint: u32 = cu;
            var consumed_units: u32 = 1;

            if (cu >= 0xD800 and cu <= 0xDFFF) {
                if (cu > 0xDBFF) {
                    // Invalid low surrogate
                    break;
                }
                if (input_idx + 1 >= w_available_len) break;

                const low = self.readUtf16Unit2Byte() orelse break;
                if (low < 0xDC00 or low > 0xDFFF) break;

                codepoint = combineSurrogates16(@intCast(cu), @intCast(low));
                consumed_units = 2;
            }

            // Only write if buffer has space
            if (output_idx < output_capacity) {
                pwz_buffer.?[output_idx] = codepoint;
                output_idx += 1;
            } else {
                buffer_full = true;
            }

            input_idx += consumed_units;
        }

        // Skip any remaining unconsumed bytes
        if (input_idx < w_available_len) {
            const skip_bytes = (w_available_len - input_idx) * 2;
            _ = self.readBufferSkip(skip_bytes);
        }

        pwz_buffer.?[output_idx] = 0;
        final_buffer_len.* = output_idx;
        return @as([*:0]const u32, @ptrCast(pwz_buffer.?));
    }

    pub fn stringReadWide(
        self: *CPackage,
        allocator: std.mem.Allocator,
        w_available_len: u16,
        encoding: u8,
        wz_static_buffer: ?[*]u32,
        static_buffer_len: u32,
        wz_buffer: ?[*]u32,
        final_buffer_len: *u32,
    ) ?[*:0]const u32 {
        var p_buffer = wz_buffer;

        if (w_available_len == 0) {
            // Empty string path
            if (static_buffer_len == 0 or wz_static_buffer == null) {
                if (final_buffer_len.* != 0 and p_buffer != null) {
                    p_buffer.?[0] = 0;
                    final_buffer_len.* = 0;
                    return @ptrCast(p_buffer.?);
                }

                final_buffer_len.* = 0;
                const new_buf = allocator.alloc(u32, 1) catch return null;
                new_buf[0] = 0;
                return @ptrCast(new_buf.ptr);
            }

            wz_static_buffer.?[0] = 0;
            final_buffer_len.* = 0;
            return @ptrCast(wz_static_buffer.?);
        }

        const available_len: u64 = w_available_len;

        switch (encoding) {
            1 => {
                return self.stringReadWideUtf8ToUtf32(allocator, w_available_len, wz_static_buffer, static_buffer_len, wz_buffer, final_buffer_len);
            },
            2 => {
                return self.stringReadWideUtf16WideToUtf32(allocator, w_available_len, wz_static_buffer, static_buffer_len, wz_buffer, final_buffer_len);
            },
            3 => {
                // Encoding 3: raw u32 (wchar_t) array
                if (static_buffer_len != 0 and static_buffer_len >= w_available_len) {
                    // Static buffer is large enough
                    _ = self.read(
                        @as([*]u8, @ptrCast(wz_static_buffer.?))[0 .. available_len * 4],
                        @intCast(available_len * 4),
                    );
                    wz_static_buffer.?[w_available_len] = 0;
                    final_buffer_len.* = @intCast(available_len);
                    return @ptrCast(wz_static_buffer.?);
                }

                // Static buffer too small or absent — zero it out if present
                if (wz_static_buffer) |static_buf| {
                    const zero_len = (static_buffer_len * 4) + 4;
                    @memset(@as([*]u8, @ptrCast(static_buf))[0..zero_len], 0);
                }

                // Allocate output buffer if not provided
                if (p_buffer == null) {
                    final_buffer_len.* = @intCast(available_len);
                    const new_buf = allocator.alloc(u32, available_len + 1) catch return null;
                    p_buffer = new_buf.ptr;
                }

                const final_len: u16 = @truncate(final_buffer_len.*);

                if (w_available_len > final_len) {
                    // Read only up to final_len, skip the rest
                    const read_len: u64 = final_len;
                    _ = self.read(
                        @as([*]u8, @ptrCast(p_buffer.?))[0 .. read_len * 4],
                        @intCast(read_len * 4),
                    );
                    p_buffer.?[final_len] = 0;
                    final_buffer_len.* = @intCast(read_len);

                    // Skip remaining bytes
                    const skip = (available_len - read_len) * 4;
                    _ = self.readBufferSkip(@intCast(skip));

                    return @ptrCast(p_buffer.?);
                }

                // Read full available
                _ = self.read(
                    @as([*]u8, @ptrCast(p_buffer.?))[0 .. available_len * 4],
                    @intCast(available_len * 4),
                );
                p_buffer.?[w_available_len] = 0;
                final_buffer_len.* = @intCast(available_len);
                return @ptrCast(p_buffer.?);
            },
            4 => {
                return self.stringReadWideUtf16ToUtf32(allocator, w_available_len, wz_static_buffer, static_buffer_len, wz_buffer, final_buffer_len);
            },
            5 => {
                return self.stringReadTruncatedUTF16(allocator, w_available_len, wz_buffer, final_buffer_len);
            },
            else => {
                std.log.warn("stringReadWide: unknown encoding {d}", .{encoding});
                return null;
            },
        }
    }
};
