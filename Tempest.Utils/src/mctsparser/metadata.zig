const std = @import("std");
const fs = std.fs;

const fnv1_32 = @import("utils.zig").fnv1_32;

pub const FieldType = enum(u16) {
    unknown = 1,
    byte,
    unsigned,
    short,
    int,
    float,
    double,
    long,
    id,
    datetime,
    // unknown,
    string = 0xC,
    dataset,
    guid,
    blob,
    account_id = 0x0209,
    character_id = 0x0309,
    clan_id = 0x0409,
    channel_id = 0x0509,
    instance_id = 0x0709,
    match_id = 0x0809,
    player_id = 0x0909,
    queue_id = 0x0A09,
    server_id = 0x0B09,
    team_id = 0x0C09,
};

pub const Field = struct {
    index: usize,
    header: u16,
    type: FieldType,
    name: []const u8,

    pub fn init(allocator: std.mem.Allocator, file_path: fs.File) ![]Field {
        var buffer: [4096]u8 = undefined;
        var fr = file_path.reader(&buffer);
        const reader = &fr.interface;

        const file_stat = try file_path.stat();
        const file_buffer = try reader.readAlloc(allocator, file_stat.size);

        return parse(allocator, file_buffer);
    }

    fn parse(allocator: std.mem.Allocator, file_buffer: []u8) ![]Field {
        var fields = std.ArrayList(Field).empty;
        defer fields.deinit(allocator);

        var fr = std.Io.Reader.fixed(file_buffer);
        var reader = &fr;

        var index: usize = 0;
        while (true) {
            const header = reader.takeInt(u16, .big) catch |err| switch (err) {
                error.EndOfStream => break,
                else => |e| return e,
            };

            const @"type": u16 = reader.takeInt(u16, .big) catch break;
            const name = try reader.takeDelimiter('\x00');

            try fields.append(allocator, .{
                .index = index,
                .header = header,
                .type = @enumFromInt(@"type"),
                .name = name.?,
            });

            index += 1;
        }

        return fields.toOwnedSlice(allocator);
    }
};

pub const Function = struct {
    index: usize,
    hash: u32,
    header: u32,
    name: []const u8,

    pub fn init(allocator: std.mem.Allocator, file_path: fs.File) ![]Function {
        var buffer: [4096]u8 = undefined;
        var fr = file_path.reader(&buffer);
        const reader = &fr.interface;

        const file_stat = try file_path.stat();
        const file_buffer = try reader.readAlloc(allocator, file_stat.size);

        return parse(allocator, file_buffer);
    }

    fn parse(allocator: std.mem.Allocator, file_buffer: []u8) ![]Function {
        var fields = std.ArrayList(Function).empty;
        defer fields.deinit(allocator);

        var fr = std.Io.Reader.fixed(file_buffer);
        var reader = &fr;

        var index: usize = 0;
        while (true) {
            const header = reader.takeInt(u32, .big) catch |err| switch (err) {
                error.EndOfStream => break,
                else => |e| return e,
            };

            const name = try reader.takeDelimiter('\x00');

            try fields.append(allocator, .{
                .index = index,
                .hash = fnv1_32(name.?),
                .header = header,
                .name = name.?,
            });

            index += 1;
        }

        return fields.toOwnedSlice(allocator);
    }
};
