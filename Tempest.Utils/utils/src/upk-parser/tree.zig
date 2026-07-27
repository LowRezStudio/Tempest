const std = @import("std");
const Io = std.Io;
const Parser = @import("parser.zig");

const ChildrenMap = std.AutoHashMap(i32, std.ArrayList(i32));

pub fn buildChildrenMap(parser: *const Parser, allocator: std.mem.Allocator) !ChildrenMap {
    var map = ChildrenMap.init(allocator);
    errdefer {
        var it = map.valueIterator();
        while (it.next()) |list| list.deinit(allocator);
        map.deinit();
    }

    for (parser.imports_table, 0..) |imp, i| {
        const self_idx: i32 = -(@as(i32, @intCast(i)) + 1);
        const gop = try map.getOrPut(imp.outer_index);
        if (!gop.found_existing) gop.value_ptr.* = .empty;
        try gop.value_ptr.append(allocator, self_idx);
    }
    for (parser.exports_table, 0..) |exp, i| {
        const self_idx: i32 = @as(i32, @intCast(i)) + 1;
        const gop = try map.getOrPut(exp.outer_index);
        if (!gop.found_existing) gop.value_ptr.* = .empty;
        try gop.value_ptr.append(allocator, self_idx);
    }

    return map;
}

const Item = union(enum) {
    detail: []const u8,
    child: i32,
};

pub fn printTree(parser: *const Parser, map: *ChildrenMap, writer: *Io.Writer, allocator: std.mem.Allocator) !void {
    const roots = map.get(0) orelse return;
    for (roots.items, 0..) |idx, i| {
        try printNode(parser, map, idx, "", i == roots.items.len - 1, writer, allocator);
    }
}

fn printNode(
    parser: *const Parser,
    map: *ChildrenMap,
    index: i32,
    prefix: []const u8,
    is_last: bool,
    writer: *Io.Writer,
    allocator: std.mem.Allocator,
) !void {
    const connector = if (is_last) "└── " else "├── ";
    const child_prefix = try std.fmt.allocPrint(allocator, "{s}{s}", .{ prefix, if (is_last) "    " else "│   " });
    defer allocator.free(child_prefix);

    var items: std.ArrayList(Item) = .empty;
    defer {
        for (items.items) |item| if (item == .detail) allocator.free(item.detail);
        items.deinit(allocator);
    }

    switch (parser.resolveIndex(index)) {
        .none => return,
        .import => |imp| {
            const name = try parser.resolveName(imp.object_name.index);
            try writer.print("{s}{s}{s} ({d})\n", .{ prefix, connector, name, index });

            try items.append(allocator, .{ .detail = try std.fmt.allocPrint(
                allocator,
                "Class:  {s}.{s}",
                .{ try parser.resolveName(imp.class_package.index), try parser.resolveName(imp.class_name.index) },
            ) });

            const outer = switch (parser.resolveIndex(imp.outer_index)) {
                .none => "None",
                .import => |o| try parser.resolveName(o.object_name.index),
                .@"export" => |o| try parser.resolveName(o.object_name.index),
            };
            try items.append(allocator, .{ .detail = try std.fmt.allocPrint(allocator, "Outer:  {s}", .{outer}) });
        },
        .@"export" => |exp| {
            const name = try parser.resolveName(exp.object_name.index);
            try writer.print("{s}{s}{s} ({d})\n", .{ prefix, connector, name, index });

            try items.append(allocator, .{ .detail = try std.fmt.allocPrint(
                allocator,
                "GUID:        {f}",
                .{exp.package_guid},
            ) });

            try items.append(allocator, .{ .detail = try std.fmt.allocPrint(allocator, "ObjectFlags: 0x{X:0>8}", .{exp.object_flags}) });

            const class_name = switch (parser.resolveIndex(exp.class_index)) {
                .none => "None",
                .import => |imp| try parser.resolveName(imp.object_name.index),
                .@"export" => |e| try parser.resolveName(e.object_name.index),
            };
            try items.append(allocator, .{ .detail = try std.fmt.allocPrint(allocator, "Class:       {s}", .{class_name}) });

            try items.append(allocator, .{ .detail = try std.fmt.allocPrint(
                allocator,
                "Size:        {d}",
                .{exp.serial_size},
            ) });
            try items.append(allocator, .{ .detail = try std.fmt.allocPrint(
                allocator,
                "Offset:      {d}",
                .{exp.serial_offset},
            ) });
        },
    }

    if (map.get(index)) |children| {
        for (children.items) |c| try items.append(allocator, .{ .child = c });
    }

    for (items.items, 0..) |item, i| {
        const last = i == items.items.len - 1;
        switch (item) {
            .detail => |d| try writer.print("{s}{s}{s}\n", .{ child_prefix, if (last) "└── " else "├── ", d }),
            .child => |c| try printNode(parser, map, c, child_prefix, last, writer, allocator),
        }
    }
}
