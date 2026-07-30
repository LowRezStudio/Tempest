const std = @import("std");

const unreal = @import("unreal.zig");
const FString = unreal.FString;
const FGuid = unreal.FGuid;

pub const package_file_tag = 0x9E2A83C1;
pub const package_file_tag_swapped = 0xC1832A9E;

pub const Error = unreal.Error || error{
    UnsupportedTag,
    UnsupportedVersion,
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

    pub fn deinit(self: FPackageFileSummary, allocator: std.mem.Allocator) void {
        allocator.free(self.folder_name.data);
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
        };
    }

    pub fn getVersion(self: FPackageFileSummary) struct { epic: i16, licensee: i16 } {
        return .{
            .epic = @intCast(self.file_version >> 16),
            .licensee = @intCast(self.file_version & 0xFFFF),
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
        , .{
            self.tag,
            self.getVersion(),
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
        });
    }
};
