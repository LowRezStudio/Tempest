const std = @import("std");
const fs = std.fs;

const tokens = @import("tokens.zig");
const Tokens = @import("tokens.zig").Tokens;

pub const CMarshalRow = struct {
    entry_count: u32,
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

    pub fn freeEntryChain(allocator: std.mem.Allocator, head: ?*CMarshalEntry) !bool {
        if (head == null) return true;

        var current = head;

        if (current.?.field_id <= Tokens.global.fields.list.len) {
            while (current) |entry| {
                const next = entry.next;

                if (entry.field_id > Tokens.global.fields.list.len) {
                    std.debug.panic("Corrupt marshal entry detected: field_id {d} > max {d}", .{
                        entry.field_id,
                        Tokens.global.fields.list.len,
                    });
                }

                entry.deinit(allocator);

                if (next == null) return true;

                current = next;
            }
        }

        @panic("Corrupt marshal entry chain detected");
    }

    pub fn clear(self: *CMarshalRow, allocator: std.mem.Allocator) void {
        if (self.entry_list) |list| {
            if (!try freeEntryChain(allocator, list)) {
                std.log.err("Avoiding freeing a marshal with bad memory.", .{});
                return;
            }
            self.entry_list = null;
        }

        if (self.entry_pool) |pool| {
            if (!try freeEntryChain(allocator, pool)) {
                std.log.err("Avoiding freeing a marshal with bad memory.", .{});
                return;
            }
            self.entry_pool = null;
        }

        self.entry_count = 0;
        self.entry_tail = null;
    }

    pub fn format(
        self: @This(),
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        try writer.print(
            \\CMarshalRowSet dump:
            \\  entries: {d}
            \\  entry_list: 0x{x}
            \\  entry_tail: 0x{x}
            \\  entry_pool: 0x{x}
            \\
        , .{
            self.entry_count,
            @intFromPtr(self.entry_list),
            @intFromPtr(self.entry_tail),
            @intFromPtr(self.entry_pool),
        });
    }
};

pub const CMarshal = struct {
    base: CMarshalRow,

    p_detail: ?*tokens.FunctionDetail,
    flags: u8,
    function_id: u32,

    pub fn init(function_id: u32) CMarshal {
        const row = CMarshalRow.init();
        return CMarshal{
            .base = row,
            .p_detail = null,
            .flags = 0,
            .function_id = function_id,
        };
    }

    pub fn clear(self: *CMarshal, allocator: std.mem.Allocator) !void {
        try self.base.clear(allocator);
        self.flags = 0;
        self.function_id = 0;
        self.p_detail = null;
    }

    pub fn load(self: *CMarshal, package: *CPackage) !usize {
        _ = self;
        _ = package;

        // @compileError("TODO: implement CMarshal.load");
        return 1;
    }

    pub fn format(
        self: @This(),
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        try writer.print(
            \\CMarshal dump:
            \\  {f}
            \\  p_detail: 0x{x}
            \\  flags: {d}
            \\  function_id: {d}
            \\
        , .{
            self.base,
            @intFromPtr(self.p_detail),
            self.flags,
            self.function_id,
        });
    }
};

pub const CMarshalRowSet = struct {
    rows: []CMarshalRow,
};

pub const CMarshalEntry = struct {
    data: union {
        dw_number: u32,
        n_number: i32,
        f_number: f32,
        d_number: f64,
        qw_number: u64,
        wz_local: [4]u8,
        wz_pointer: ?*const u8,
        by_data: ?*const u8,
        row_set: ?*CMarshalRowSet,
    },
    next: ?*CMarshalEntry,
    field_id: u16,
    size: u16,

    pub fn init(allocator: *std.mem.Allocator) CMarshalEntry {
        _ = allocator;

        @compileError("TODO: implement CMarshalEntry.init");
    }

    pub fn deinit(self: *CMarshalEntry, allocator: *std.mem.Allocator) void {
        _ = self;
        _ = allocator;

        @compileError("TODO: implement CMarshalEntry.deinit");
    }

    pub fn clear(self: *CMarshalEntry, allocator: *std.mem.Allocator) void {
        _ = self;
        _ = allocator;

        @compileError("TODO: implement CMarshalEntry.clear");
    }

    pub fn format(
        self: @This(),
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        try writer.print(
            \\CMarshalEntry dump:
            \\  data: 0x{any}
            \\  next: 0x{x}
            \\  field_id: {d}
            \\  size: {d}
            \\
        , .{
            self.data,
            @intFromPtr(self.next),
            self.field_id,
            self.size,
        });
    }
};

pub const CMscEntry = struct {
    next: ?*CMscEntry,

    pub fn format(
        self: @This(),
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        try writer.print(
            \\CMscEntry dump:
            \\  next: 0x{x}
            \\
        , .{@intFromPtr(self.next)});
    }
};

pub const CPackPacketNET = struct {
    header: extern union {
        fields: packed struct {
            size: u16,
            extended: u8,
            flags: u8,
        },
        all: u32,
    },
    data: [0x7fe]u8,

    pub fn format(
        self: @This(),
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        try writer.print(
            \\CPackPacketNET dump:
            \\  header: {any}
            \\  data: ...
            \\
        , .{
            @intFromPtr(&self.header),
        });
    }
};

pub const CPackPacket = struct {
    next: ?*CPackPacket,
    net: CPackPacketNET,
    source_func: u32,

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

    pub fn freeChain(allocator: std.mem.Allocator, chain: ?*CPackPacket) void {
        if (chain == null) return;

        var current = chain;
        while (current) |packet| {
            const next = packet.next;
            packet.deinit(allocator);
            current = next;
        }
    }

    pub fn format(
        self: @This(),
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        try writer.print(
            \\CPackPacket dump:
            \\  next: 0x{x}
            \\  net: {f}
            \\  source_func: {d}
            \\
        , .{
            @intFromPtr(self.next),
            self.net,
            self.source_func,
        });
    }
};

pub const CPackage = struct {
    base: CMscEntry,

    packets: ?*CPackPacket,
    used: u32,
    place: u32,
    current: ?*CPackPacket,
    cur_place: u32,
    first_alloc: bool,
    flags: u8,
    extended: u8,
    p_extended: [0xc]u8,
    encoding: u8,
    db_action_func: u32,

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
        CPackPacket.freeChain(allocator, self.packets);
        self.packets = null;
        self.current = null;
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

    pub fn write(self: *CPackage, allocator: std.mem.Allocator, buffer: [*]const u8, size: u32) !u64 {
        if (size == 0) return 0;

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

    pub fn format(
        self: @This(),
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        try writer.print(
            \\CPackage dump:
            \\  {f}
            \\  packets: 0x{x}
            \\  used: {d}
            \\  place: {d}
            \\  current: 0x{x}
            \\  cur_place: {d}
            \\  first_alloc: {d}
            \\  flags: {d}
            \\  extended: {d}
            \\  p_extended: {any}
            \\  encoding: {d}
            \\  db_action_func: {d}
            \\
        , .{
            self.base,
            @intFromPtr(self.packets),
            self.used,
            self.place,
            @intFromPtr(self.current),
            self.cur_place,
            @intFromBool(self.first_alloc),
            self.flags,
            self.extended,
            self.p_extended,
            self.encoding,
            self.db_action_func,
        });

        return writer.writeAll("\n");
    }
};
