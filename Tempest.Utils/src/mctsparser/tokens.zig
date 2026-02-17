const std = @import("std");
const fs = std.fs;

pub const Tokens = struct {
    functions: Functions,
    fields: Fields,

    pub fn init(allocator: std.mem.Allocator, functions: []FunctionDetail, fields: []FieldEntry) !void {
        global = .{
            .functions = try Functions.init(allocator, functions),
            .fields = Fields.init(fields),
        };
    }

    pub var global: @This() = undefined;
};

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
    index: u32,
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

pub const Functions = struct {
    hashmap: std.AutoHashMap(u32, FunctionDetail),
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator, list: []FunctionDetail) !Functions {
        var hashmap = std.AutoHashMap(u32, FunctionDetail).init(allocator);
        for (list) |function| {
            try hashmap.put(function.index, function);
        }

        return .{
            .hashmap = hashmap,
            .allocator = allocator,
        };
    }

    pub fn deinit(self: *Functions) void {
        self.hashmap.deinit();
    }

    pub fn findByIndex(self: *const Functions, hash: u32) ?FunctionDetail {
        if (self.hashmap.get(hash)) |value| {
            return value;
        }
        return null;
    }
};

pub const Fields = struct {
    list: []FieldEntry,

    pub fn init(list: []FieldEntry) Fields {
        return .{
            .list = list,
        };
    }

    pub fn findByIndex(self: *const Fields, index: usize) ?FieldEntry {
        if (index < self.list.len) {
            return self.list[index];
        }
        return null;
    }
};
