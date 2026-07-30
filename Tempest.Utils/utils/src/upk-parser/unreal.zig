const std = @import("std");

pub const package_file_tag = 0x9E2A83C1;
pub const package_file_tag_swapped = 0xC1832A9E;

pub const Error = std.Io.Reader.Error || std.Io.Reader.TakeEnumError || std.mem.Allocator.Error || error{
    UnsupportedTag,
    UnsupportedVersion,
    UnsupportedEncoding,
    InvalidArraySize,
};

pub fn takeArray(
    comptime T: type,
    reader: *std.Io.Reader,
    allocator: std.mem.Allocator,
) Error![]T {
    const count = try reader.takeInt(i32, .little);
    if (count == 0) return &.{};
    if (count < 0) return error.InvalidArraySize;

    const array = try allocator.alloc(T, @intCast(count));
    errdefer allocator.free(array);

    for (array) |*item| {
        item.* = try T.take(reader, allocator);
    }

    return array;
}

pub const FString = struct {
    data: []const u8,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!FString {
        const count = try reader.takeInt(i32, .little);
        if (count == 0) return .{ .data = try allocator.alloc(u8, 0) };

        // ANSICHAR
        if (count > 0) {
            const len: usize = @intCast(count);
            const data = try reader.readAlloc(allocator, @intCast(len));
            defer allocator.free(data);

            // strip trailing null
            const str_len = if (len > 0 and data[len - 1] == 0) len - 1 else len;
            return .{ .data = try allocator.dupe(u8, data[0..str_len]) };
        } else {
            // WIDECHAR
            return error.UnsupportedEncoding;
        }
    }
};

pub const FGuid = extern struct {
    a: i32,
    b: i32,
    c: i32,
    d: i32,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!FGuid {
        _ = allocator;
        return try reader.takeStruct(FGuid, .little);
    }

    pub fn eql(a: FGuid, b: FGuid) bool {
        return a.a == b.a and a.b == b.b and a.c == b.c and a.d == b.d;
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

pub const EPackageFlags = packed struct(u32) {
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

pub const ECompressionFlags = packed struct(u32) {
    type: enum(u3) {
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

    pub fn format(self: @This(), writer: *std.Io.Writer) std.Io.Writer.Error!void {
        try writer.print("{s}", .{@tagName(self.type)});

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

pub const FGenerationInfo = extern struct {
    export_count: i32,
    name_count: i32,
    net_object_count: i32,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!FGenerationInfo {
        _ = allocator;
        return try reader.takeStruct(FGenerationInfo, .little);
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

pub const FCompressedChunk = extern struct {
    uncompressed_offset: i32,
    uncompressed_size: i32,
    compressed_offset: i32,
    compressed_size: i32,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!FCompressedChunk {
        _ = allocator;
        return try reader.takeStruct(FCompressedChunk, .little);
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

pub const EPixelFormat = enum(u32) {
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

pub const FTextureAllocations = struct {
    size_x: i32,
    size_y: i32,
    num_mips: i32,
    tex_format: EPixelFormat,
    text_create_flags: u32,
    export_indices: []i32,

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!FTextureAllocations {
        const size_x = try reader.takeInt(i32, .little);
        const size_y = try reader.takeInt(i32, .little);
        const num_mips = try reader.takeInt(i32, .little);
        const tex_format = try reader.takeEnum(EPixelFormat, .little);
        const text_create_flags = try reader.takeInt(u32, .little);

        const num_export_indices = try reader.takeInt(u32, .little);
        if (num_export_indices > 0) {
            const indices = try allocator.alloc(i32, num_export_indices);
            errdefer allocator.free(indices);

            for (indices) |*index| {
                index.* = try reader.takeInt(i32, .little);
            }

            return .{
                .size_x = size_x,
                .size_y = size_y,
                .num_mips = num_mips,
                .tex_format = tex_format,
                .text_create_flags = text_create_flags,
                .export_indices = indices,
            };
        }

        return .{
            .size_x = size_x,
            .size_y = size_y,
            .num_mips = num_mips,
            .tex_format = tex_format,
            .text_create_flags = text_create_flags,
            .export_indices = &.{},
        };
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
            \\    text_create_flags: 0x{X:0>8}
            \\
        , .{
            self.size_x,
            self.size_y,
            self.num_mips,
            @as(u32, @bitCast(self.tex_format)),
            @tagName(self.tex_format),
            self.text_create_flags,
        });

        if (self.export_indices.len > 0) {
            try writer.print("    export_indices ({d}):\n", .{self.export_indices.len});
            for (self.export_indices) |index| {
                try writer.print("      {d}\n", .{index});
            }
        }
    }
};

pub const FPackageFileSummary = struct {
    tag: u32,
    file_version: i32,
    total_header_size: i32,
    folder_name: FString,
    package_flags: EPackageFlags,
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
    guid: FGuid,
    generations: []FGenerationInfo,
    engine_version: i32,
    cooked_content_version: i32,
    compression_flags: ECompressionFlags,
    compressed_chunks: []FCompressedChunk,
    package_source: u32,
    additional_packages_to_cook: []FString,
    texture_allocations: []FTextureAllocations,

    pub fn deinit(self: FPackageFileSummary, allocator: std.mem.Allocator) void {
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
                allocator.free(allocation.export_indices);
            }
            allocator.free(self.texture_allocations);
        }
    }

    pub fn take(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error!FPackageFileSummary {
        const tag = try reader.takeInt(u32, .little);
        if (tag != package_file_tag) {
            return error.UnsupportedTag;
        }

        const file_version = try reader.takeInt(i32, .little);
        if (file_version & 0xFFFF < 870) return error.UnsupportedVersion;

        const total_header_size = try reader.takeInt(i32, .little);
        const folder_name = try FString.take(reader, allocator);
        const package_flags = try reader.takeStruct(EPackageFlags, .little);
        const name_count = try reader.takeInt(i32, .little);
        const name_offset = try reader.takeInt(i32, .little);
        const export_count = try reader.takeInt(i32, .little);
        const export_offset = try reader.takeInt(i32, .little);
        const import_count = try reader.takeInt(i32, .little);
        const import_offset = try reader.takeInt(i32, .little);
        const depends_offset = try reader.takeInt(i32, .little);
        const import_export_guids_offset = try reader.takeInt(i32, .little);
        const import_guids_count = try reader.takeInt(i32, .little);
        const export_guids_count = try reader.takeInt(i32, .little);
        const thumbnail_table_offset = try reader.takeInt(i32, .little);
        const guid = try FGuid.take(reader, allocator);
        const generations = try takeArray(FGenerationInfo, reader, allocator);
        const engine_version = try reader.takeInt(i32, .little);
        const cooked_content_version = try reader.takeInt(i32, .little);
        const compression_flags = try reader.takeStruct(ECompressionFlags, .little);
        const compressed_chunks = try takeArray(FCompressedChunk, reader, allocator);
        const package_source = try reader.takeInt(u32, .little);
        const additional_packages_to_cook = try takeArray(FString, reader, allocator);
        const texture_allocations = try takeArray(FTextureAllocations, reader, allocator);

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

    pub fn getFileVersion(self: FPackageFileSummary) struct { version: i16, licensee: i16 } {
        return .{
            .version = @intCast(self.file_version & 0xFFFF),
            .licensee = @intCast(self.file_version >> 16),
        };
    }

    pub fn getCookedContentVersion(self: FPackageFileSummary) struct { version: i16, licensee: i16 } {
        return .{
            .version = @intCast(self.cooked_content_version & 0xFFFF),
            .licensee = @intCast(self.cooked_content_version >> 16),
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
