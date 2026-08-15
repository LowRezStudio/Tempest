const std = @import("std");

const unreal = @import("unreal");

pub const Error = unreal.Error || error{
    CorruptProperty,
    PropertyStreamOob,
};

/// Cap on properties parsed from a single stream.
/// Guards against data that never reaches a None terminator.
const max_properties: usize = 10_000;

/// Name/object resolution context provided by the host parser. The parser wires
/// its own name-map / import-export-map resolvers in so this module stays
/// decoupled from `Parser`.
pub const Ctx = struct {
    ctx: *const anyopaque,
    allocator: std.mem.Allocator,
    /// Format a Name (name index + instance number) into `buf`, returning a
    /// slice borrowed from `buf` or the name map.
    formatName: *const fn (*const anyopaque, unreal.Name, []u8) []const u8,
    /// Resolve a package object index (0 -> "None", >0 export, <0 import) into
    /// display text.
    resolveObject: *const fn (*const anyopaque, i32, []u8) []const u8,
};

/// The decoded value of a tagged property.
pub const Value = union(enum) {
    none,
    int: i32,
    float: f32,
    boolean: bool,
    /// Plain byte (no enum).
    byte: u8,
    /// Enum-backed byte: `value_name` is the Name of the enum entry.
    enum_byte: struct { enum_name: unreal.Name, value_name: unreal.Name },
    name: unreal.Name,
    /// Owned string (decoded from the String in the blob).
    string: []const u8,
    /// Package object index (0 -> None, >0 export, <0 import).
    object: i32,
    delegate: struct { object: i32, function: unreal.Name },
    /// Array elements are opaque: the inner property type is script metadata,
    /// not present in the tag, so `data` (borrowed from the blob) is kept raw.
    array: struct { count: i32, data: []const u8 },
    /// Script struct serialized as a nested tagged property list.
    tagged_struct: struct { struct_name: unreal.Name, members: []Property },
    /// Native/immutable struct serialized as raw bytes (borrowed from blob).
    raw_struct: struct { struct_name: unreal.Name, data: []const u8 },
    /// Non-core / unimplemented property type: bytes skipped by size, kept raw.
    unknown: struct { type_name: unreal.Name, data: []const u8 },
};

pub const Property = struct {
    name: unreal.Name,
    type_name: unreal.Name,
    array_index: i32,
    size: i32,
    /// Absolute image offset of this property's tag (the Name field).
    offset: usize,
    value: Value,
};

pub const ParseResult = struct {
    net_index: i32,
    properties: []Property,
    /// True when the stream did not cleanly reach a None terminator.
    truncated: bool,
    /// Bytes of the property region consumed (through the None terminator),
    /// not counting the net-index prefix.
    property_bytes: usize,
};

/// Parse an export's object data: a net-index INT prefix followed by a tagged
/// property stream terminated by a None tag. `base` is the image offset of
/// `blob[0]`, used for absolute tag offsets in the report.
pub fn parseExport(blob: []const u8, base: usize, ctx: *const Ctx, allocator: std.mem.Allocator) Error!ParseResult {
    if (blob.len < 4) {
        return .{ .net_index = 0, .properties = &.{}, .truncated = true, .property_bytes = 0 };
    }
    const net_index = std.mem.readInt(i32, blob[0..4], .little);

    var list = try std.ArrayList(Property).initCapacity(allocator, 0);
    defer list.deinit(allocator);

    const consumed: ?usize = parseTagged(blob[4..], base + 4, ctx, allocator, &list) catch |err| switch (err) {
        error.OutOfMemory => return error.OutOfMemory,
        else => null,
    };

    const properties = try list.toOwnedSlice(allocator);
    return .{
        .net_index = net_index,
        .properties = properties,
        .truncated = consumed == null,
        .property_bytes = consumed orelse 0,
    };
}

pub fn deinitProperties(properties: []Property, allocator: std.mem.Allocator) void {
    for (properties) |prop| deinitProperty(prop, allocator);
    if (properties.len > 0) allocator.free(properties);
}

fn deinitProperty(prop: Property, allocator: std.mem.Allocator) void {
    switch (prop.value) {
        .string => |s| allocator.free(s),
        .tagged_struct => |ts| deinitProperties(ts.members, allocator),
        else => {},
    }
}

/// Parse a tagged property stream starting at `slice[0]`, appending to `list`.
/// Returns the number of bytes consumed through the None terminator on success,
/// or an error if the stream is corrupt / never terminates.
fn parseTagged(
    slice: []const u8,
    base: usize,
    ctx: *const Ctx,
    allocator: std.mem.Allocator,
    list: *std.ArrayList(Property),
) Error!usize {
    var off: usize = 0;
    var reader: std.Io.Reader = .fixed(slice);
    var buf: [256]u8 = undefined;

    while (off + 8 <= slice.len and list.items.len < max_properties) {
        const tag_offset = off;
        const name = try unreal.Name.take(&reader, allocator);
        off += 8;

        // Terminator: the Name whose resolved string is "None".
        if (std.mem.eql(u8, ctx.formatName(ctx.ctx, name, &buf), "None")) return off;

        if (off + 16 > slice.len) return error.PropertyStreamOob;
        const type_name = try unreal.Name.take(&reader, allocator);
        off += 8;
        const size = try reader.takeInt(i32, .little);
        const array_index = try reader.takeInt(i32, .little);
        off += 8;
        if (size < 0) return error.CorruptProperty;

        const type_str = ctx.formatName(ctx.ctx, type_name, &buf);

        var struct_name: unreal.Name = .{ .name_index = 0, .number = 0 };
        var bool_val: u8 = 0;
        var enum_name: unreal.Name = .{ .name_index = 0, .number = 0 };
        if (std.mem.eql(u8, type_str, "StructProperty")) {
            struct_name = try unreal.Name.take(&reader, allocator);
            off += 8;
        } else if (std.mem.eql(u8, type_str, "BoolProperty")) {
            bool_val = try reader.takeInt(u8, .little);
            off += 1;
        } else if (std.mem.eql(u8, type_str, "ByteProperty")) {
            enum_name = try unreal.Name.take(&reader, allocator);
            off += 8;
        }

        const value_slice = try reader.take(@intCast(size));
        off += @intCast(size);

        const prop = try parseValue(
            ctx,
            allocator,
            base,
            name,
            type_name,
            array_index,
            size,
            tag_offset,
            value_slice,
            struct_name,
            bool_val,
            enum_name,
        );
        try list.append(allocator, prop);
    }
    if (list.items.len >= max_properties) return error.PropertyStreamOob;
    return error.PropertyStreamOob;
}

fn parseValue(
    ctx: *const Ctx,
    allocator: std.mem.Allocator,
    base: usize,
    name: unreal.Name,
    type_name: unreal.Name,
    array_index: i32,
    size: i32,
    tag_offset: usize,
    value_slice: []const u8,
    struct_name: unreal.Name,
    bool_val: u8,
    enum_name: unreal.Name,
) Error!Property {
    var buf: [256]u8 = undefined;
    const type_str = ctx.formatName(ctx.ctx, type_name, &buf);

    const template = Property{
        .name = name,
        .type_name = type_name,
        .array_index = array_index,
        .size = size,
        .offset = base + tag_offset,
        .value = undefined,
    };

    if (std.mem.eql(u8, type_str, "IntProperty")) {
        if (value_slice.len < 4) return error.CorruptProperty;
        return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .int = std.mem.readInt(i32, value_slice[0..4], .little) } };
    }
    if (std.mem.eql(u8, type_str, "FloatProperty")) {
        if (value_slice.len < 4) return error.CorruptProperty;
        const bits = std.mem.readInt(u32, value_slice[0..4], .little);
        return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .float = @bitCast(bits) } };
    }
    if (std.mem.eql(u8, type_str, "BoolProperty")) {
        return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .boolean = bool_val != 0 } };
    }
    if (std.mem.eql(u8, type_str, "ByteProperty")) {
        if (std.mem.eql(u8, ctx.formatName(ctx.ctx, enum_name, &buf), "None")) {
            if (value_slice.len < 1) return error.CorruptProperty;
            return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .byte = value_slice[0] } };
        }
        if (value_slice.len < 8) return error.CorruptProperty;
        return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .enum_byte = .{ .enum_name = enum_name, .value_name = readName(value_slice) } } };
    }
    if (std.mem.eql(u8, type_str, "NameProperty")) {
        if (value_slice.len < 8) return error.CorruptProperty;
        return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .name = readName(value_slice) } };
    }
    if (std.mem.eql(u8, type_str, "StrProperty")) {
        var reader: std.Io.Reader = .fixed(value_slice);
        const fs = try unreal.String.take(&reader, allocator);
        return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .string = fs.data } };
    }
    if (std.mem.eql(u8, type_str, "ObjectProperty") or
        std.mem.eql(u8, type_str, "ClassProperty") or
        std.mem.eql(u8, type_str, "ComponentProperty") or
        std.mem.eql(u8, type_str, "InterfaceProperty"))
    {
        if (value_slice.len < 4) return error.CorruptProperty;
        return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .object = std.mem.readInt(i32, value_slice[0..4], .little) } };
    }
    if (std.mem.eql(u8, type_str, "DelegateProperty")) {
        if (value_slice.len < 12) return error.CorruptProperty;
        return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .delegate = .{
            .object = std.mem.readInt(i32, value_slice[0..4], .little),
            .function = readName(value_slice[4..]),
        } } };
    }
    if (std.mem.eql(u8, type_str, "ArrayProperty")) {
        if (value_slice.len < 4) return error.CorruptProperty;
        return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .array = .{
            .count = std.mem.readInt(i32, value_slice[0..4], .little),
            .data = value_slice[4..],
        } } };
    }
    if (std.mem.eql(u8, type_str, "StructProperty")) {
        var members = try std.ArrayList(Property).initCapacity(allocator, 0);
        const consumed = parseTagged(value_slice, base + tag_offset, ctx, allocator, &members) catch {
            // Native/immutable struct (or corrupt): keep the raw bytes.
            deinitProperties(members.items, allocator);
            members.deinit(allocator);
            return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .raw_struct = .{ .struct_name = struct_name, .data = value_slice } } };
        };
        if (consumed == value_slice.len) {
            const owned = try members.toOwnedSlice(allocator);
            members.deinit(allocator);
            return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .tagged_struct = .{ .struct_name = struct_name, .members = owned } } };
        }
        // Tagged parse succeeded but didn't span the whole body: keep raw.
        deinitProperties(members.items, allocator);
        members.deinit(allocator);
        return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .raw_struct = .{ .struct_name = struct_name, .data = value_slice } } };
    }
    if (std.mem.eql(u8, type_str, "MapProperty")) {
        return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .none };
    }

    // Non-core / unimplemented type: bytes already skipped by size, kept raw.
    return .{ .name = template.name, .type_name = template.type_name, .array_index = template.array_index, .size = template.size, .offset = template.offset, .value = .{ .unknown = .{ .type_name = type_name, .data = value_slice } } };
}

fn readName(bytes: []const u8) unreal.Name {
    return .{
        .name_index = std.mem.readInt(i32, bytes[0..4], .little),
        .number = std.mem.readInt(i32, bytes[4..8], .little),
    };
}

// ---------------------------------------------------------------------------
// Report printing
// ---------------------------------------------------------------------------

fn paddedName(buf: *[32]u8, s: []const u8) []const u8 {
    const n = @min(s.len, buf.len);
    @memcpy(buf[0..n], s[0..n]);
    @memset(buf[n..], ' ');
    return buf[0..buf.len];
}

/// Print one property line plus indented recursion for tagged structs.
fn printProperty(prop: Property, ctx: *const Ctx, indent: usize) void {
    var namebuf: [128]u8 = undefined;
    var pad: [32]u8 = undefined;
    var vbuf: [2048]u8 = undefined;
    var typebuf: [128]u8 = undefined;
    var tabsbuf: [64]u8 = undefined;

    const name_str = ctx.formatName(ctx.ctx, prop.name, &namebuf);
    const type_str = ctx.formatName(ctx.ctx, prop.type_name, &typebuf);
    const pad_len = @min(indent, tabsbuf.len);
    @memset(tabsbuf[0..pad_len], '\t');
    const tabs = tabsbuf[0..pad_len];
    const value_text = formatValue(prop, ctx, &vbuf);

    std.debug.print("{s}{s} {s}  [{s} size={d} @{d}]\n", .{
        tabs,
        paddedName(&pad, name_str),
        value_text,
        type_str,
        prop.size,
        prop.offset,
    });

    if (prop.value == .tagged_struct) {
        printProperties(prop.value.tagged_struct.members, ctx, indent + 1);
    }
}

pub fn printProperties(properties: []const Property, ctx: *const Ctx, indent: usize) void {
    for (properties) |prop| printProperty(prop, ctx, indent);
}

fn formatValue(prop: Property, ctx: *const Ctx, buf: []u8) []const u8 {
    switch (prop.value) {
        .none => return "none",
        .int => |v| return std.fmt.bufPrint(buf, "{d}", .{v}) catch "?",
        .float => |v| return std.fmt.bufPrint(buf, "{d:.6}", .{v}) catch "?",
        .boolean => |v| return if (v) "True" else "False",
        .byte => |v| return std.fmt.bufPrint(buf, "{d}", .{v}) catch "?",
        .enum_byte => |v| {
            var name_buf: [128]u8 = undefined;
            return std.fmt.bufPrint(buf, "{s}", .{ctx.formatName(ctx.ctx, v.value_name, &name_buf)}) catch "?";
        },
        .name => |v| {
            var name_buf: [128]u8 = undefined;
            return std.fmt.bufPrint(buf, "{s}", .{ctx.formatName(ctx.ctx, v, &name_buf)}) catch "?";
        },
        .string => |s| return capped(buf, s, 200),
        .object => |idx| {
            var name_buf: [128]u8 = undefined;
            return std.fmt.bufPrint(buf, "{s}", .{ctx.resolveObject(ctx.ctx, idx, &name_buf)}) catch "?";
        },
        .delegate => |d| {
            var name_buf: [128]u8 = undefined;
            var function_buf: [128]u8 = undefined;
            return std.fmt.bufPrint(buf, "{s}.{s}", .{
                ctx.resolveObject(ctx.ctx, d.object, &name_buf),
                ctx.formatName(ctx.ctx, d.function, &function_buf),
            }) catch "?";
        },
        .array => |arr| {
            var hex_buf: [48]u8 = undefined;
            const preview = hexPreviewOf(arr.data, 8, &hex_buf);
            if (preview.len == 0) {
                return std.fmt.bufPrint(buf, "[{d} elements, {d} bytes]", .{ arr.count, arr.data.len }) catch "?";
            }
            return std.fmt.bufPrint(buf, "[{d} elements, {d} bytes] {s}", .{ arr.count, arr.data.len, preview }) catch "?";
        },
        .tagged_struct => |ts| {
            var name_buf: [128]u8 = undefined;
            return std.fmt.bufPrint(buf, "{s} ({d} members)", .{ ctx.formatName(ctx.ctx, ts.struct_name, &name_buf), ts.members.len }) catch "?";
        },
        .raw_struct => |rs| {
            var name_buf: [128]u8 = undefined;
            return std.fmt.bufPrint(buf, "{s} (raw {d} bytes)", .{ ctx.formatName(ctx.ctx, rs.struct_name, &name_buf), rs.data.len }) catch "?";
        },
        .unknown => |u| {
            var name_buf: [128]u8 = undefined;
            return std.fmt.bufPrint(buf, "{s} (skipped {d} bytes)", .{ ctx.formatName(ctx.ctx, u.type_name, &name_buf), u.data.len }) catch "?";
        },
    }
}

fn capped(buf: []u8, s: []const u8, max: usize) []const u8 {
    if (s.len <= max) return s;
    return std.fmt.bufPrint(buf, "{s}...", .{s[0..max]}) catch "?";
}

fn hexPreviewOf(data: []const u8, max: usize, buf: []u8) []const u8 {
    if (data.len == 0) return "";
    const n = @min(data.len, max);
    const hex = "0123456789abcdef";
    var off: usize = 0;
    for (data[0..n]) |b| {
        if (off + 3 > buf.len) break;
        buf[off] = hex[b >> 4];
        buf[off + 1] = hex[b & 0xf];
        off += 2;
        buf[off] = ' ';
        off += 1;
    }
    return buf[0..off];
}

/// Count occurrences of skipped (non-core) property types in a property tree.
pub const SkipCounter = struct {
    map: std.StringHashMap(u32),
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator) SkipCounter {
        return .{ .map = std.StringHashMap(u32).init(allocator), .allocator = allocator };
    }

    pub fn deinit(self: *SkipCounter) void {
        var it = self.map.iterator();
        while (it.next()) |e| self.allocator.free(e.key_ptr.*);
        self.map.deinit();
    }

    pub fn collect(self: *SkipCounter, properties: []const Property, ctx: *const Ctx) void {
        for (properties) |prop| {
            switch (prop.value) {
                .unknown => |u| {
                    var buf: [128]u8 = undefined;
                    const key = self.allocator.dupe(u8, ctx.formatName(ctx.ctx, u.type_name, &buf)) catch return;
                    const gop = self.map.getOrPut(key) catch {
                        self.allocator.free(key);
                        return;
                    };
                    if (gop.found_existing) {
                        self.allocator.free(key);
                        gop.value_ptr.* += 1;
                    } else {
                        gop.value_ptr.* = 1;
                    }
                },
                .tagged_struct => |ts| self.collect(ts.members, ctx),
                else => {},
            }
        }
    }
};

const test_names = [_][]const u8{
    "None", // 0
    "MyInt", // 1
    "IntProperty", // 2
    "MyStr", // 3
    "StrProperty", // 4
    "MyStruct", // 5
    "StructProperty", // 6
    "NavigationQueueEntry", // 7
    "MemberA", // 8
    "MemberB", // 9
    "FloatProperty", // 10
    "MyBool", // 11
    "BoolProperty", // 12
};

fn testFormatName(ctx: *const anyopaque, name: unreal.Name, buf: []u8) []const u8 {
    _ = ctx;
    _ = buf;
    if (name.name_index < 0 or name.name_index >= test_names.len) return "<bad>";
    return test_names[@intCast(name.name_index)];
}

fn testResolveObject(ctx: *const anyopaque, index: i32, buf: []u8) []const u8 {
    _ = ctx;
    _ = buf;
    if (index == 0) return "None";
    return "obj";
}

test "parse a synthetic tagged property stream" {
    const a = std.testing.allocator;

    const ctx = Ctx{
        .ctx = undefined,
        .allocator = a,
        .formatName = &testFormatName,
        .resolveObject = &testResolveObject,
    };

    // Build a blob: net_index + tags + None terminator.
    var out: std.Io.Writer.Allocating = .init(a);
    defer out.deinit();
    const w = &out.writer;
    try w.writeInt(i32, 0, .little); // net_index

    // MyInt / IntProperty / size=4 / arr=0 / value=42
    try w.writeInt(i32, 1, .little); // name index "MyInt"
    try w.writeInt(i32, 0, .little);
    try w.writeInt(i32, 2, .little); // type "IntProperty"
    try w.writeInt(i32, 0, .little);
    try w.writeInt(i32, 4, .little); // size
    try w.writeInt(i32, 0, .little); // array index
    try w.writeInt(i32, 42, .little);

    // MyStruct / StructProperty / size=N / struct_name=NavigationQueueEntry / nested tags
    // Build nested first so we know its size.
    var nested: std.Io.Writer.Allocating = .init(a);
    defer nested.deinit();
    const nw = &nested.writer;
    // MemberA / FloatProperty / size=4 / value=1.0
    try nw.writeInt(i32, 8, .little);
    try nw.writeInt(i32, 0, .little);
    try nw.writeInt(i32, 10, .little);
    try nw.writeInt(i32, 0, .little);
    try nw.writeInt(i32, 4, .little);
    try nw.writeInt(i32, 0, .little);
    try nw.writeInt(u32, @bitCast(@as(f32, 1.0)), .little);
    // MemberB / IntProperty / size=4 / value=7
    try nw.writeInt(i32, 9, .little);
    try nw.writeInt(i32, 0, .little);
    try nw.writeInt(i32, 2, .little);
    try nw.writeInt(i32, 0, .little);
    try nw.writeInt(i32, 4, .little);
    try nw.writeInt(i32, 0, .little);
    try nw.writeInt(i32, 7, .little);
    // None terminator
    try nw.writeInt(i32, 0, .little);
    try nw.writeInt(i32, 0, .little);
    const nested_bytes = try nested.toOwnedSlice();
    defer a.free(nested_bytes);

    try w.writeInt(i32, 5, .little); // name "MyStruct"
    try w.writeInt(i32, 0, .little);
    try w.writeInt(i32, 6, .little); // type "StructProperty"
    try w.writeInt(i32, 0, .little);
    try w.writeInt(i32, @intCast(nested_bytes.len), .little); // size
    try w.writeInt(i32, 0, .little); // array index
    try w.writeInt(i32, 7, .little); // struct_name "NavigationQueueEntry"
    try w.writeInt(i32, 0, .little);
    try w.writeAll(nested_bytes);

    // MyBool / BoolProperty / size=0 / boolval=1
    try w.writeInt(i32, 11, .little); // name "MyBool"
    try w.writeInt(i32, 0, .little);
    try w.writeInt(i32, 12, .little); // type "BoolProperty"
    try w.writeInt(i32, 0, .little);
    try w.writeInt(i32, 0, .little); // size 0
    try w.writeInt(i32, 0, .little); // array index
    try w.writeByte(1); // BoolVal

    // None terminator
    try w.writeInt(i32, 0, .little);
    try w.writeInt(i32, 0, .little);

    const blob = try out.toOwnedSlice();
    defer a.free(blob);

    const result = try parseExport(blob, 0, &ctx, a);
    defer deinitProperties(result.properties, a);

    try std.testing.expectEqual(@as(i32, 0), result.net_index);
    try std.testing.expect(!result.truncated);
    try std.testing.expectEqual(@as(usize, 3), result.properties.len);

    // Int
    try std.testing.expectEqual(@as(i32, 42), result.properties[0].value.int);

    // Struct -> tagged, 2 members
    const s = result.properties[1].value;
    try std.testing.expect(s == .tagged_struct);
    try std.testing.expectEqual(@as(usize, 2), s.tagged_struct.members.len);
    try std.testing.expectEqual(@as(f32, 1.0), s.tagged_struct.members[0].value.float);

    // Bool
    try std.testing.expectEqual(true, result.properties[2].value.boolean);
}
