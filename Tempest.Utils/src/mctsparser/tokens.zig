const std = @import("std");
const fs = std.fs;

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
    index: usize,

    sort_index: u16,
    netid_type: NetIdType,
    type: FieldType,
    name: []const u8,

    pub fn init(allocator: std.mem.Allocator, file_path: fs.File) ![]FieldEntry {
        var buffer: [4096]u8 = undefined;
        var fr = file_path.reader(&buffer);
        const reader = &fr.interface;

        const file_stat = try file_path.stat();
        const file_buffer = try reader.readAlloc(allocator, file_stat.size);

        return deserialize(allocator, file_buffer);
    }

    fn deserialize(allocator: std.mem.Allocator, file_buffer: []u8) ![]FieldEntry {
        var fields = std.ArrayList(FieldEntry).empty;
        defer fields.deinit(allocator);

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
    index: usize,
    hash: u32,

    sort_index: u16,
    flags: u16,
    name: []const u8,
    wide_name: []const u16,

    pub fn init(allocator: std.mem.Allocator, file_path: fs.File) ![]FunctionDetail {
        var buffer: [4096]u8 = undefined;
        var fr = file_path.reader(&buffer);
        const reader = &fr.interface;

        const file_stat = try file_path.stat();
        const file_buffer = try reader.readAlloc(allocator, file_stat.size);

        return deserialize(allocator, file_buffer);
    }

    fn deserialize(allocator: std.mem.Allocator, file_buffer: []u8) ![]FunctionDetail {
        var fields = std.ArrayList(FunctionDetail).empty;
        defer fields.deinit(allocator);

        var fr = std.Io.Reader.fixed(file_buffer);
        var reader = &fr;

        var index: usize = 0;
        while (true) {
            const sort_index = reader.takeInt(u16, .big) catch |err| switch (err) {
                error.EndOfStream => break,
                else => |e| return e,
            };
            const flags = reader.takeInt(u16, .big) catch break;

            const name = try reader.takeDelimiter('\x00');

            try fields.append(allocator, .{
                .index = index,
                .hash = fnv1_32(name.?),
                .sort_index = sort_index,
                .flags = flags,
                .name = name.?,
                .wide_name = &.{},
            });

            index += 1;
        }

        return fields.toOwnedSlice(allocator);
    }
};
