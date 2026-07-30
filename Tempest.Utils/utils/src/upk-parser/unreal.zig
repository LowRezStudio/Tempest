const std = @import("std");

pub const package_file_tag = 0x9E2A83C1;
pub const package_file_tag_swapped = 0xC1832A9E;

pub const Error = std.Io.Reader.Error || std.mem.Allocator.Error || error{
    UnsupportedTag,
    UnsupportedVersion,
    UnsupportedEncoding,
    InvalidArraySize,
};

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

    pub fn take(reader: *std.Io.Reader) Error!FGuid {
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

pub const FGenerationInfo = extern struct {
    export_count: i32,
    name_count: i32,
    net_object_count: i32,

    pub fn take(reader: *std.Io.Reader) Error!FGenerationInfo {
        return try reader.takeStruct(FGenerationInfo, .little);
    }

    pub fn takeArray(reader: *std.Io.Reader, allocator: std.mem.Allocator) Error![]FGenerationInfo {
        const count = try reader.takeInt(i32, .little);
        if (count == 0) return &.{};
        if (count < 0) return error.InvalidArraySize;

        const generations = try allocator.alloc(FGenerationInfo, @intCast(count));
        errdefer allocator.free(generations);

        const self = @This();
        for (generations) |*generation| {
            generation.* = try self.take(reader);
        }

        return generations;
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

    pub fn deinit(self: FPackageFileSummary, allocator: std.mem.Allocator) void {
        allocator.free(self.folder_name.data);
        if (self.generations.len > 0) {
            allocator.free(self.generations);
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
        const guid = try FGuid.take(reader);
        const generations = try FGenerationInfo.takeArray(reader, allocator);
        const engine_version = try reader.takeInt(i32, .little);
        const cooked_content_version = try reader.takeInt(i32, .little);

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
        });

        if (self.generations.len > 0) {
            try writer.print("  generations ({d}):\n", .{self.generations.len});
            for (self.generations) |generation| {
                try writer.print("{f}\n", .{generation});
            }
        }
    }
};
