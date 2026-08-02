const std = @import("std");

pub const package_file_tag = 0x9E2A83C1;
pub const package_file_tag_swapped = 0xC1832A9E;

pub const min_supported_version = 800;
pub const ver_guid_maps = 622;
pub const ver_thumbnails = 583;
pub const ver_additional_packages_to_cook = 515;
pub const ver_texture_allocations_save = 766;
pub const ver_texture_allocations_load = 892;

pub const Error = std.Io.Reader.Error || std.Io.Reader.TakeEnumError || std.unicode.Utf16LeToUtf8AllocError || error{
    UnsupportedTag,
    UnsupportedVersion,
    UnsupportedEndianness,
    UnsupportedEncoding,
    InvalidArraySize,
};

pub const WriteError = std.Io.Writer.Error || std.mem.Allocator.Error || error{
    InvalidUtf8,
};

/// Serialized as `ArrayNum (INT)` followed by `ArrayNum` element serializations.
pub fn takeArray(
    comptime T: type,
    reader: *std.Io.Reader,
    allocator: std.mem.Allocator,
) Error![]T {
    const count = try reader.takeInt(i32, .little);
    if (count < 0) return error.InvalidArraySize;
    if (count == 0) return &.{};

    const array = try allocator.alloc(T, @intCast(count));
    errdefer allocator.free(array);

    if (comptime @typeInfo(T) == .int) {
        for (array) |*item| {
            item.* = try reader.takeInt(T, .little);
        }
    } else {
        for (array) |*item| {
            item.* = try T.take(reader, allocator);
        }
    }

    return array;
}

pub fn writeArray(
    comptime T: type,
    writer: *std.Io.Writer,
    array: []const T,
    allocator: std.mem.Allocator,
) WriteError!void {
    try writer.writeInt(i32, @intCast(array.len), .little);

    if (comptime @typeInfo(T) == .int) {
        for (array) |item| {
            try writer.writeInt(T, item, .little);
        }
    } else {
        for (array) |*item| {
            try T.write(item.*, writer, allocator);
        }
    }
}

/// Write `name` in UE3's length-prefixed string form shared by String and
/// NameEntry: positive count = ANSI bytes, negative count = UTF-16 code
/// units, and the count always includes the trailing null terminator.
fn writeName(writer: *std.Io.Writer, allocator: std.mem.Allocator, name: []const u8) WriteError!void {
    var wide = false;
    for (name) |c| {
        if (c >= 0x80) {
            wide = true;
            break;
        }
    }

    if (wide) {
        const units = try std.unicode.utf8ToUtf16LeAlloc(allocator, name);
        defer allocator.free(units);
        try writer.writeInt(i32, -@as(i32, @intCast(units.len + 1)), .little);
        for (units) |u| {
            try writer.writeInt(u16, u, .little);
        }
        try writer.writeInt(u16, 0, .little);
    } else {
        try writer.writeInt(i32, @intCast(name.len + 1), .little);
        try writer.writeAll(name);
        try writer.writeByte(0);
    }
}

/// Read `len` ANSI bytes, dropping the trailing `\0` that UE3 always serializes.
fn takeAnsiName(reader: *std.Io.Reader, allocator: std.mem.Allocator, len: usize) Error![]u8 {
    if (len == 0) return allocator.alloc(u8, 0);

    const raw = try reader.readAlloc(allocator, len);
    defer allocator.free(raw);

    const str_len = if (raw[len - 1] == 0) len - 1 else len;
    return allocator.dupe(u8, raw[0..str_len]);
}

/// Read `units` UTF-16-LE code units (2 bytes each), decoding to UTF-8 and
/// dropping the trailing null code unit UE3 serializes.
fn takeWideName(reader: *std.Io.Reader, allocator: std.mem.Allocator, units: usize) Error![]u8 {
    if (units == 0) return allocator.alloc(u8, 0);

    const utf16 = try allocator.alloc(u16, units);
    defer allocator.free(utf16);

    for (utf16) |*unit| {
        unit.* = try reader.takeInt(u16, .little);
    }

    const real_units = if (utf16[units - 1] == 0) units - 1 else units;
    return std.unicode.utf16LeToUtf8Alloc(allocator, utf16[0..real_units]);
}

pub const String = struct {
    /// Owned UTF-8, trailing null stripped.
    data: []const u8,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!String {
        const count = try reader.takeInt(i32, .little);
        if (count == std.math.minInt(i32)) return error.InvalidArraySize;
        if (count == 0) return .{ .data = try allocator.alloc(u8, 0) };

        // count < 0 means the string was stored as UTF-16.
        return .{ .data = if (count > 0)
            try takeAnsiName(reader, allocator, @intCast(count))
        else
            try takeWideName(reader, allocator, @intCast(-@as(i64, count))) };
    }

    pub fn write(self: String, writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        try writeName(writer, allocator, self.data);
    }
};

pub const Guid = extern struct {
    a: i32,
    b: i32,
    c: i32,
    d: i32,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!Guid {
        _ = allocator;
        return try reader.takeStruct(Guid, .little);
    }

    pub fn write(self: Guid, writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        _ = allocator;
        try writer.writeStruct(self, .little);
    }

    pub fn eql(x: Guid, y: Guid) bool {
        return x.a == y.a and x.b == y.b and x.c == y.c and x.d == y.d;
    }

    pub fn format(
        self: @This(),
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        const a: u32 = @bitCast(self.a);
        const b: u32 = @bitCast(self.b);
        const c: u32 = @bitCast(self.c);
        const d: u32 = @bitCast(self.d);

        try writer.print("{X:0>8}-{X:0>4}-{X:0>4}-{X:0>4}-{X:0>12}", .{
            a,
            b >> 16,
            b & 0xFFFF,
            c >> 16,
            (@as(u64, c & 0xFFFF) << 32) | @as(u64, d),
        });
    }
};

pub const PackageFlags = packed struct(u32) {
    allow_download: bool = false,
    client_optional: bool = false,
    server_side_only: bool = false,
    cooked: bool = false,
    unsecure: bool = false,
    saved_with_newer_version: bool = false,
    _pad1: u9 = 0,
    need: bool = false,
    compiling: bool = false,
    contains_map: bool = false,
    trash: bool = false,
    disallow_lazy_loading: bool = false,
    play_in_editor: bool = false,
    contains_script: bool = false,
    contains_debug_info: bool = false,
    require_imports_already_loaded: bool = false,
    _pad2: u1 = 0,
    store_compressed: bool = false,
    store_fully_compressed: bool = false,
    _pad3: u1 = 0,
    contains_face_fx_data: bool = false,
    no_export_allowed: bool = false,
    stripped_source: bool = false,
    filter_editor_only: bool = false,

    pub fn write(self: @This(), writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        _ = allocator;
        try writer.writeStruct(self, .little);
    }

    pub fn format(self: @This(), writer: *std.Io.Writer) std.Io.Writer.Error!void {
        const info = @typeInfo(@This()).@"struct";

        var i: usize = 0;
        inline for (info.field_names, info.field_types) |name, field_type| {
            if (comptime std.mem.startsWith(u8, name, "_pad")) continue;
            if (field_type == bool) {
                if (@field(self, name)) {
                    try writer.print("{s}{s}", .{
                        if (i == 0) "" else ", ",
                        name,
                    });
                    i += 1;
                }
            }
        }
    }
};

pub const CompressionFlags = packed struct(u32) {
    codec: enum(u3) {
        none = 0,
        zlib = 1,
        lzo = 2,
        lzx = 4,
    } = .none,
    _pad1: u1 = 0,
    bias_memory: bool = false,
    bias_speed: bool = false,
    _pad2: u1 = 0,
    force_ppu_decompress_zlib: bool = false,
    no_stats: bool = false,
    obscured: bool = false,
    _pad3: u22 = 0,

    pub fn write(self: @This(), writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        _ = allocator;
        try writer.writeStruct(self, .little);
    }

    pub fn format(self: @This(), writer: *std.Io.Writer) std.Io.Writer.Error!void {
        try writer.print("{s}", .{@tagName(self.codec)});

        const info = @typeInfo(@This()).@"struct";
        inline for (info.field_names, info.field_types) |name, field_type| {
            if (comptime std.mem.startsWith(u8, name, "_pad")) continue;
            if (field_type == bool) {
                if (@field(self, name)) {
                    try writer.print(", {s}", .{name});
                }
            }
        }
    }
};

pub const GenerationInfo = extern struct {
    export_count: i32,
    name_count: i32,
    net_object_count: i32,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!GenerationInfo {
        _ = allocator;
        return try reader.takeStruct(GenerationInfo, .little);
    }

    pub fn write(self: GenerationInfo, writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        _ = allocator;
        try writer.writeStruct(self, .little);
    }

    pub fn format(
        self: @This(),
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        try writer.print(
            \\    export_count: {d}
            \\    name_count: {d}
            \\    net_object_count: {d}
            \\
        , .{
            self.export_count,
            self.name_count,
            self.net_object_count,
        });
    }
};

pub const CompressedChunk = extern struct {
    uncompressed_offset: i32,
    uncompressed_size: i32,
    compressed_offset: i32,
    compressed_size: i32,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!CompressedChunk {
        _ = allocator;
        return try reader.takeStruct(CompressedChunk, .little);
    }

    pub fn write(self: CompressedChunk, writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        _ = allocator;
        try writer.writeStruct(self, .little);
    }

    pub fn format(
        self: @This(),
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        try writer.print(
            \\    uncompressed_offset: {d}
            \\    uncompressed_size: {d}
            \\    compressed_offset: {d}
            \\    compressed_size: {d}
            \\
        , .{
            self.uncompressed_offset,
            self.uncompressed_size,
            self.compressed_offset,
            self.compressed_size,
        });
    }
};

/// Name is a reference into the package name map: an index + instance number.
/// Resolve the base string via the parsed name map (see Parser.resolveName).
pub const Name = struct {
    name_index: i32,
    number: i32,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!Name {
        _ = allocator;
        return .{
            .name_index = try reader.takeInt(i32, .little),
            .number = try reader.takeInt(i32, .little),
        };
    }

    pub fn write(self: Name, writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        _ = allocator;
        try writer.writeInt(i32, self.name_index, .little);
        try writer.writeInt(i32, self.number, .little);
    }

    pub fn format(self: @This(), writer: *std.Io.Writer) std.Io.Writer.Error!void {
        if (self.number == 0) {
            try writer.print("{d}", .{self.name_index});
        } else {
            try writer.print("{d}_{d}", .{ self.name_index, self.number - 1 });
        }
    }
};

/// One entry of the name map. Serialized as StringLen (INT, negative = UTF-16),
/// the string bytes (null-terminated on disk), then an 8-byte Flags field
pub const NameEntry = struct {
    /// Owned UTF-8, trailing null stripped.
    name: []const u8,
    flags: u64,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!NameEntry {
        const string_len = try reader.takeInt(i32, .little);
        if (string_len == std.math.minInt(i32)) return error.InvalidArraySize;

        const name = if (string_len >= 0)
            try takeAnsiName(reader, allocator, @intCast(string_len))
        else
            try takeWideName(reader, allocator, @intCast(-@as(i64, string_len)));
        errdefer allocator.free(name);

        const flags = try reader.takeInt(u64, .little);
        return .{ .name = name, .flags = flags };
    }

    pub fn write(self: NameEntry, writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        try writeName(writer, allocator, self.name);
        try writer.writeInt(u64, self.flags, .little);
    }

    pub fn deinit(self: NameEntry, allocator: std.mem.Allocator) void {
        allocator.free(self.name);
    }
};

pub const ObjectImport = struct {
    class_package: Name,
    class_name: Name,
    outer_index: i32,
    object_name: Name,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!ObjectImport {
        return .{
            .class_package = try Name.take(reader, allocator),
            .class_name = try Name.take(reader, allocator),
            .outer_index = try reader.takeInt(i32, .little),
            .object_name = try Name.take(reader, allocator),
        };
    }

    pub fn write(self: ObjectImport, writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        try self.class_package.write(writer, allocator);
        try self.class_name.write(writer, allocator);
        try writer.writeInt(i32, self.outer_index, .little);
        try self.object_name.write(writer, allocator);
    }
};

/// ObjectExport. The LegacyComponentMap (TMap<Name,INT>)
/// only exists for file version < 543 and is never present for our range (v >= 800).
pub const ObjectExport = struct {
    class_index: i32,
    super_index: i32,
    outer_index: i32,
    object_name: Name,
    archetype_index: i32,
    object_flags: u64,
    serial_size: i32,
    serial_offset: i32,
    export_flags: u32,
    generation_net_object_count: []i32,
    package_guid: Guid,
    package_flags: u32,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!ObjectExport {
        const class_index = try reader.takeInt(i32, .little);
        const super_index = try reader.takeInt(i32, .little);
        const outer_index = try reader.takeInt(i32, .little);
        const object_name = try Name.take(reader, allocator);
        const archetype_index = try reader.takeInt(i32, .little);
        const object_flags = try reader.takeInt(u64, .little);
        const serial_size = try reader.takeInt(i32, .little);
        const serial_offset = try reader.takeInt(i32, .little);
        const export_flags = try reader.takeInt(u32, .little);
        const generation_net_object_count = try takeArray(i32, reader, allocator);
        errdefer allocator.free(generation_net_object_count);
        const package_guid = try Guid.take(reader, allocator);
        const package_flags = try reader.takeInt(u32, .little);

        return .{
            .class_index = class_index,
            .super_index = super_index,
            .outer_index = outer_index,
            .object_name = object_name,
            .archetype_index = archetype_index,
            .object_flags = object_flags,
            .serial_size = serial_size,
            .serial_offset = serial_offset,
            .export_flags = export_flags,
            .generation_net_object_count = generation_net_object_count,
            .package_guid = package_guid,
            .package_flags = package_flags,
        };
    }

    pub fn write(self: ObjectExport, writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        try writer.writeInt(i32, self.class_index, .little);
        try writer.writeInt(i32, self.super_index, .little);
        try writer.writeInt(i32, self.outer_index, .little);
        try self.object_name.write(writer, allocator);
        try writer.writeInt(i32, self.archetype_index, .little);
        try writer.writeInt(u64, self.object_flags, .little);
        try writer.writeInt(i32, self.serial_size, .little);
        try writer.writeInt(i32, self.serial_offset, .little);
        try writer.writeInt(u32, self.export_flags, .little);
        try writeArray(i32, writer, self.generation_net_object_count, allocator);
        try self.package_guid.write(writer, allocator);
        try writer.writeInt(u32, self.package_flags, .little);
    }

    pub fn deinit(self: ObjectExport, allocator: std.mem.Allocator) void {
        if (self.generation_net_object_count.len > 0) {
            allocator.free(self.generation_net_object_count);
        }
    }
};

/// One import-GUID entry: a level name and the GUIDs of objects in that level
pub const LevelGuids = struct {
    level_name: String,
    guids: []Guid,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!LevelGuids {
        return .{
            .level_name = try String.take(reader, allocator),
            .guids = try takeArray(Guid, reader, allocator),
        };
    }

    pub fn write(self: LevelGuids, writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        try self.level_name.write(writer, allocator);
        try writeArray(Guid, writer, self.guids, allocator);
    }

    pub fn deinit(self: LevelGuids, allocator: std.mem.Allocator) void {
        allocator.free(self.level_name.data);
        if (self.guids.len > 0) allocator.free(self.guids);
    }
};

/// One export-GUID entry.
pub const ExportGuid = struct {
    guid: Guid,
    export_index: i32,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!ExportGuid {
        return .{
            .guid = try Guid.take(reader, allocator),
            .export_index = try reader.takeInt(i32, .little),
        };
    }

    pub fn write(self: ExportGuid, writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        try self.guid.write(writer, allocator);
        try writer.writeInt(i32, self.export_index, .little);
    }
};

pub const PixelFormat = enum(u32) {
    unknown = 0x0,
    a32b32g32r32f = 0x1,
    a8r8g8b8 = 0x2,
    g8 = 0x3,
    g16 = 0x4,
    dxt1 = 0x5,
    dxt3 = 0x6,
    dxt5 = 0x7,
    uyvy = 0x8,
    float_rgb = 0x9,
    float_rgba = 0xa,
    depth_stencil = 0xb,
    shadow_depth = 0xc,
    filtered_shadow_depth = 0xd,
    r32f = 0xe,
    g16r16 = 0xf,
    g16r16f = 0x10,
    g16r16f_filter = 0x11,
    g32r32f = 0x12,
    a2b10g10r10 = 0x13,
    a16b16g16r16 = 0x14,
    d24 = 0x15,
    r16f = 0x16,
    r16f_filter = 0x17,
    bc5 = 0x18,
    v8u8 = 0x19,
    a1 = 0x1a,
    float_r11g11b10 = 0x1b,
    a4r4g4b4 = 0x1c,
    g8r8 = 0x1d,
    b8g8r8a8 = 0x1e,
    max = 0x1f,
};

pub fn pixelFormatName(format: u32) []const u8 {
    const info = @typeInfo(PixelFormat).@"enum";
    inline for (info.field_names, info.field_values) |name, value| {
        if (value == format) return name;
    }
    return "unknown";
}

/// A single TextureType entry. TextureAllocations serializes as a plain
/// TArray<TextureType>; the summary therefore stores
/// `texture_allocations: []TextureType`.
pub const TextureType = struct {
    size_x: i32,
    size_y: i32,
    num_mips: i32,
    pixel_format: u32,
    tex_create_flags: u32,
    export_indices: []i32,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!TextureType {
        const size_x = try reader.takeInt(i32, .little);
        const size_y = try reader.takeInt(i32, .little);
        const num_mips = try reader.takeInt(i32, .little);
        const pixel_format = try reader.takeInt(u32, .little);
        const tex_create_flags = try reader.takeInt(u32, .little);
        const export_indices = try takeArray(i32, reader, allocator);

        return .{
            .size_x = size_x,
            .size_y = size_y,
            .num_mips = num_mips,
            .pixel_format = pixel_format,
            .tex_create_flags = tex_create_flags,
            .export_indices = export_indices,
        };
    }

    pub fn write(self: TextureType, writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        try writer.writeInt(i32, self.size_x, .little);
        try writer.writeInt(i32, self.size_y, .little);
        try writer.writeInt(i32, self.num_mips, .little);
        try writer.writeInt(u32, self.pixel_format, .little);
        try writer.writeInt(u32, self.tex_create_flags, .little);
        try writeArray(i32, writer, self.export_indices, allocator);
    }

    pub fn format(
        self: @This(),
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        try writer.print(
            \\    size_x: {d}
            \\    size_y: {d}
            \\    num_mips: {d}
            \\    format: 0x{X:0>8} ({s})
            \\    tex_create_flags: 0x{X:0>8}
            \\
        , .{
            self.size_x,
            self.size_y,
            self.num_mips,
            self.pixel_format,
            pixelFormatName(self.pixel_format),
            self.tex_create_flags,
        });

        if (self.export_indices.len > 0) {
            try writer.print("    export_indices ({d}):\n", .{self.export_indices.len});
            for (self.export_indices) |index| {
                try writer.print("      {d}\n", .{index});
            }
        }
    }
};

pub const PackageFileSummary = struct {
    tag: u32,
    file_version: i32,
    total_header_size: i32,
    folder_name: String,
    package_flags: PackageFlags,
    name_count: i32,
    name_offset: i32,
    export_count: i32,
    export_offset: i32,
    import_count: i32,
    import_offset: i32,
    depends_offset: i32,
    import_export_guids_offset: i32,
    import_guids_count: i32,
    export_guids_count: i32,
    thumbnail_table_offset: i32,
    guid: Guid,
    generations: []GenerationInfo,
    engine_version: i32,
    cooked_content_version: i32,
    compression_flags: CompressionFlags,
    compressed_chunks: []CompressedChunk,
    package_source: u32,
    additional_packages_to_cook: []String,
    texture_allocations: []TextureType,

    pub fn deinit(self: PackageFileSummary, allocator: std.mem.Allocator) void {
        allocator.free(self.folder_name.data);

        if (self.generations.len > 0) {
            allocator.free(self.generations);
        }

        if (self.compressed_chunks.len > 0) {
            allocator.free(self.compressed_chunks);
        }

        if (self.additional_packages_to_cook.len > 0) {
            for (self.additional_packages_to_cook) |*package| {
                allocator.free(package.data);
            }
            allocator.free(self.additional_packages_to_cook);
        }

        if (self.texture_allocations.len > 0) {
            for (self.texture_allocations) |*allocation| {
                if (allocation.export_indices.len > 0) {
                    allocator.free(allocation.export_indices);
                }
            }
            allocator.free(self.texture_allocations);
        }
    }

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!PackageFileSummary {
        const tag = try reader.takeInt(u32, .little);
        if (tag == package_file_tag_swapped) return error.UnsupportedEndianness;
        if (tag != package_file_tag) return error.UnsupportedTag;

        const file_version = try reader.takeInt(i32, .little);
        const epic_version: i32 = file_version & 0xFFFF;
        if (epic_version < min_supported_version) return error.UnsupportedVersion;

        const total_header_size = try reader.takeInt(i32, .little);
        const folder_name = try String.take(reader, allocator);
        const package_flags = try reader.takeStruct(PackageFlags, .little);
        const name_count = try reader.takeInt(i32, .little);
        const name_offset = try reader.takeInt(i32, .little);
        const export_count = try reader.takeInt(i32, .little);
        const export_offset = try reader.takeInt(i32, .little);
        const import_count = try reader.takeInt(i32, .little);
        const import_offset = try reader.takeInt(i32, .little);
        const depends_offset = try reader.takeInt(i32, .little);

        // v > 622: cross-level reference GUID maps.
        var import_export_guids_offset: i32 = -1;
        var import_guids_count: i32 = 0;
        var export_guids_count: i32 = 0;
        if (epic_version > ver_guid_maps) {
            import_export_guids_offset = try reader.takeInt(i32, .little);
            import_guids_count = try reader.takeInt(i32, .little);
            export_guids_count = try reader.takeInt(i32, .little);
        }

        // v > 583: thumbnail table offset.
        var thumbnail_table_offset: i32 = 0;
        if (epic_version > ver_thumbnails) {
            thumbnail_table_offset = try reader.takeInt(i32, .little);
        }

        const guid = try Guid.take(reader, allocator);
        const generations = try takeArray(GenerationInfo, reader, allocator);
        const engine_version = try reader.takeInt(i32, .little);
        const cooked_content_version = try reader.takeInt(i32, .little);
        const compression_flags = try reader.takeStruct(CompressionFlags, .little);
        const compressed_chunks = try takeArray(CompressedChunk, reader, allocator);
        const package_source = try reader.takeInt(u32, .little);

        // v > 515: streaming level dependencies.
        var additional_packages_to_cook: []String = &.{};
        if (epic_version > ver_additional_packages_to_cook) {
            additional_packages_to_cook = try takeArray(String, reader, allocator);
        }

        // Paladins fork v > 892 (save would only need v > 766).
        var texture_allocations: []TextureType = &.{};
        if (epic_version > ver_texture_allocations_save and epic_version > ver_texture_allocations_load) {
            texture_allocations = try takeArray(TextureType, reader, allocator);
        }

        return .{
            .tag = tag,
            .file_version = file_version,
            .total_header_size = total_header_size,
            .folder_name = folder_name,
            .package_flags = package_flags,
            .name_count = name_count,
            .name_offset = name_offset,
            .export_count = export_count,
            .export_offset = export_offset,
            .import_count = import_count,
            .import_offset = import_offset,
            .depends_offset = depends_offset,
            .import_export_guids_offset = import_export_guids_offset,
            .import_guids_count = import_guids_count,
            .export_guids_count = export_guids_count,
            .thumbnail_table_offset = thumbnail_table_offset,
            .guid = guid,
            .generations = generations,
            .engine_version = engine_version,
            .cooked_content_version = cooked_content_version,
            .compression_flags = compression_flags,
            .compressed_chunks = compressed_chunks,
            .package_source = package_source,
            .additional_packages_to_cook = additional_packages_to_cook,
            .texture_allocations = texture_allocations,
        };
    }

    pub fn write(self: PackageFileSummary, writer: *std.Io.Writer, allocator: std.mem.Allocator) WriteError!void {
        const epic_version = self.getFileVersion().version;

        try writer.writeInt(u32, self.tag, .little);
        try writer.writeInt(i32, self.file_version, .little);
        try writer.writeInt(i32, self.total_header_size, .little);
        try self.folder_name.write(writer, allocator);
        try self.package_flags.write(writer, allocator);
        try writer.writeInt(i32, self.name_count, .little);
        try writer.writeInt(i32, self.name_offset, .little);
        try writer.writeInt(i32, self.export_count, .little);
        try writer.writeInt(i32, self.export_offset, .little);
        try writer.writeInt(i32, self.import_count, .little);
        try writer.writeInt(i32, self.import_offset, .little);
        try writer.writeInt(i32, self.depends_offset, .little);

        if (epic_version > ver_guid_maps) {
            try writer.writeInt(i32, self.import_export_guids_offset, .little);
            try writer.writeInt(i32, self.import_guids_count, .little);
            try writer.writeInt(i32, self.export_guids_count, .little);
        }

        if (epic_version > ver_thumbnails) {
            try writer.writeInt(i32, self.thumbnail_table_offset, .little);
        }

        try self.guid.write(writer, allocator);
        try writeArray(GenerationInfo, writer, self.generations, allocator);
        try writer.writeInt(i32, self.engine_version, .little);
        try writer.writeInt(i32, self.cooked_content_version, .little);
        try self.compression_flags.write(writer, allocator);
        try writeArray(CompressedChunk, writer, self.compressed_chunks, allocator);
        try writer.writeInt(u32, self.package_source, .little);

        if (epic_version > ver_additional_packages_to_cook) {
            try writeArray(String, writer, self.additional_packages_to_cook, allocator);
        }

        if (epic_version > ver_texture_allocations_save and epic_version > ver_texture_allocations_load) {
            try writeArray(TextureType, writer, self.texture_allocations, allocator);
        }
    }

    pub fn getFileVersion(self: PackageFileSummary) struct { version: u16, licensee: u16 } {
        const bits: u32 = @bitCast(self.file_version);
        return .{
            .version = @intCast(bits & 0xFFFF),
            .licensee = @intCast(bits >> 16),
        };
    }

    pub fn getCookedContentVersion(self: PackageFileSummary) struct { version: u16, licensee: u16 } {
        const bits: u32 = @bitCast(self.cooked_content_version);
        return .{
            .version = @intCast(bits & 0xFFFF),
            .licensee = @intCast(bits >> 16),
        };
    }

    pub fn format(
        self: @This(),
        writer: *std.Io.Writer,
    ) std.Io.Writer.Error!void {
        try writer.print(
            \\Package File Summary:
            \\  tag: 0x{X}
            \\  file_version: {any}
            \\  total_header_size: {d}
            \\  folder_name: {s}
            \\  package_flags: 0x{X:0>8} ({f})
            \\  name_count: {d}
            \\  name_offset: {d}
            \\  export_count: {d}
            \\  export_offset: {d}
            \\  import_count: {d}
            \\  import_offset: {d}
            \\  depends_offset: {d}
            \\  import_export_guids_offset: {d}
            \\  import_guids_count: {d}
            \\  export_guids_count: {d}
            \\  thumbnail_table_offset: {d}
            \\  guid: {f}
            \\  engine_version: {d}
            \\  cooked_content_version: {any}
            \\  compression_flags: 0x{X:0>8} ({f})
            \\  package_source: {X:0>8}
            \\
            \\
        , .{
            self.tag,
            self.getFileVersion(),
            self.total_header_size,
            self.folder_name.data,
            @as(u32, @bitCast(self.package_flags)),
            self.package_flags,
            self.name_count,
            self.name_offset,
            self.export_count,
            self.export_offset,
            self.import_count,
            self.import_offset,
            self.depends_offset,
            self.import_export_guids_offset,
            self.import_guids_count,
            self.export_guids_count,
            self.thumbnail_table_offset,
            self.guid,
            self.engine_version,
            self.getCookedContentVersion(),
            @as(u32, @bitCast(self.compression_flags)),
            self.compression_flags,
            self.package_source,
        });

        if (self.generations.len > 0) {
            try writer.print("  generations ({d}):\n", .{self.generations.len});
            for (self.generations) |generation| {
                try writer.print("{f}\n", .{generation});
            }
        }

        if (self.compressed_chunks.len > 0) {
            try writer.print("  compressed_chunks ({d}):\n", .{self.compressed_chunks.len});
            for (self.compressed_chunks) |chunk| {
                try writer.print("{f}\n", .{chunk});
            }
        }

        if (self.additional_packages_to_cook.len > 0) {
            try writer.print("  additional_packages_to_cook ({d}):\n", .{self.additional_packages_to_cook.len});
            for (self.additional_packages_to_cook) |package| {
                try writer.print("    {s}\n", .{package.data});
            }
        }

        if (self.texture_allocations.len > 0) {
            try writer.print("  texture_allocations ({d}):\n", .{self.texture_allocations.len});
            for (self.texture_allocations) |allocation| {
                try writer.print("{f}\n", .{allocation});
            }
        }
    }
};
