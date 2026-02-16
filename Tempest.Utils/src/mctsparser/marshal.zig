const std = @import("std");
const fs = std.fs;

pub const CMarshalRow = struct {
    entry_count: u32,
    entry_list: ?*CMarshalEntry,
    entry_tail: ?*CMarshalEntry,
    entry_pool: ?*CMarshalEntry,
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
    size: u16,
};

pub const CMscEntry = struct {
    next: ?*CMscEntry,
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

    pub fn freeChain(allocator: std.mem.Allocator, p_chain: ?*CPackPacket) void {
        if (p_chain == null) return;

        var current = p_chain;
        while (current) |packet| {
            const next = packet.next;
            packet.deinit(allocator);
            current = next;
        }
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
