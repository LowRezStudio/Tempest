const std = @import("std");
const fs = std.fs;

const UpkParser = @import("utils").upkparser;
const Objects = @import("utils").upkparser.Objects;
const Archive = @import("utils").upkparser.Archive;
const Core = @import("utils").upkparser.Core;

pub fn patchSFSC(parser: *UpkParser.Parser) !bool {
    var none_name_idx: u32 = 0;
    for (parser.names_table, 0..) |name_entry, i| {
        if (std.mem.eql(u8, name_entry.name.toString(), "None\x00")) {
            none_name_idx = @intCast(i);
            break;
        }
    }

    const SFSCExport = struct {
        index: u32,
        object: Archive.FObjectExport,
    };

    var sfsc_exports = std.ArrayList(SFSCExport).empty;
    errdefer sfsc_exports.deinit(parser.allocator);

    for (parser.exports_table, 0..) |exp, i| {
        if (std.mem.eql(u8, parser.names_table[exp.object_name].name.toString(), "SeekFreeShaderCache\x00")) {
            // std.log.info("Found SeekFreeShaderCache export at index {d}", .{i});
            try sfsc_exports.append(parser.allocator, .{
                .index = @intCast(i),
                .object = exp,
            });
        }
    }

    if (sfsc_exports.items.len == 0) {
        // std.log.err("No SeekFreeShaderCache exports found! skipping", .{});
        return false;
    }

    // Patch the SFSC exports
    for (parser.exports_table[1..], 1..) |exp, i| {
        _ = exp;

        for (sfsc_exports.items) |sfsc_export| {
            if (i == sfsc_export.index) {
                parser.exports_table[i].class_index = 0;
                parser.exports_table[i].super_index = 0;
                parser.exports_table[i].outer_index = 0;
                parser.exports_table[i].object_name = @intCast(none_name_idx);
                parser.exports_table[i].archetype_index = 0;
                parser.exports_table[i].archetype = 0;
                parser.exports_table[i].object_flags = 0;
                parser.exports_table[i].serial_size = 0;
                parser.exports_table[i].export_flags = 0;
                parser.exports_table[i].generation_net_object_count = 0;
                parser.exports_table[i].package_guid = .{};
                parser.exports_table[i].package_flags = 0;
                break;
            }
        }
    }

    return true;
}

// Get file name without extension and path
pub fn getFileName(path: []const u8) []const u8 {
    const file = blk: {
        if (std.mem.lastIndexOfScalar(u8, path, '/')) |idx| break :blk path[idx + 1 ..];
        if (std.mem.lastIndexOfScalar(u8, path, '\\')) |idx| break :blk path[idx + 1 ..];
        break :blk path;
    };

    if (std.mem.lastIndexOfScalar(u8, file, '.')) |dot| {
        return file[0..dot];
    }

    return file;
}

pub fn patchGuidCache(allocator: std.mem.Allocator, files: [][]const u8) !bool {
    var guids_map = std.StringArrayHashMap(Core.FGuid).init(allocator);

    var parser: UpkParser.Parser = undefined;

    for (files) |file| {
        const f = try fs.cwd().openFile(file, .{});
        defer f.close();

        parser = try UpkParser.Parser.init(allocator, f, .{});
        try parser.parse();

        const file_name = getFileName(file);
        const file_guid = parser.summary.guid;
        try guids_map.put(file_name, file_guid);

        // Try to find any exports with a guid
        var package_class_index: i32 = 0;
        for (parser.imports_table[1..], 1..) |imp, i| {
            if (std.mem.eql(u8, parser.names_table[imp.object_name].name.toString(), "Package\x00")) {
                package_class_index = @intCast(i);
                break;
            }
        }

        for (parser.exports_table[1..], 1..) |exp, i| {
            // guid could be empty
            // if (Core.FGuid.eql(exp.package_guid, Core.FGuid{})) continue;
            if (exp.class_index == -package_class_index) {
                try guids_map.put(parser.names_table[exp.object_name].name.toString(), parser.exports_table[i].package_guid);
            }
        }
    }

    const guid_cache = for (files) |file| {
        if (file.len >= "guidcache.upk".len) {
            const suffix = file[file.len - "guidcache.upk".len ..];
            if (std.ascii.eqlIgnoreCase(suffix, "guidcache.upk"))
                break file;
        }
    } else return false;

    const file = try fs.cwd().openFile(guid_cache, .{});
    defer file.close();

    parser = try UpkParser.Parser.init(allocator, file, .{});
    try parser.parse();

    const cache_export = parser.exports_table[1];

    var fr = std.Io.Reader.fixed(parser.file_buffer);
    var reader = &fr;

    reader.seek = cache_export.serial_offset;

    const net_index = try reader.takeInt(i32, .little);
    const obj_name = try Core.FName.take(reader, parser.allocator, false);

    const caches = try Objects.UGuidCache.takeArray(reader, parser.allocator);

    // debug print the guids map
    // var iter = guids_map.iterator();
    // while (iter.next()) |entry| {
    //     const key = entry.key_ptr.*;
    //     const value = entry.value_ptr.*;
    //     std.debug.print("Key: {s}, Value: {f}\n", .{ key, value });
    // }
    // std.debug.print("len: {d}\n", .{guids_map.count()});

    // Find any mismatching guids
    for (caches) |*cache| {
        const name = parser.names_table[cache.name.index].name.toString();
        if (guids_map.get(name)) |g| {
            if (!Core.FGuid.eql(cache.guid, g)) {
                // Patch the guids
                std.debug.print("Patching {s} ({f} -> {f})\n", .{ name, cache.guid, g });
                cache.guid = g;
            }
        }
    }

    // Write the new guid cache
    var fw = std.Io.Writer.fixed(parser.data_buffer);
    var writer = &fw;

    // writer.end = cache_export.serial_offset;

    try writer.writeInt(i32, net_index, .little);
    try Core.FName.write(obj_name, writer, false);
    try Objects.UGuidCache.writeArray(caches, writer);
    try writer.flush();

    try parser.save(guid_cache);

    return true;
}
