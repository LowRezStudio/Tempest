const std = @import("std");
const mem = std.mem;

pub const Constants = @import("constants.zig");
pub const ECompressionFlags = @import("flags.zig").ECompressionFlags;
pub const EObjectFlags = @import("flags.zig").EObjectFlags;
pub const EPackageFlags = @import("flags.zig").EPackageFlags;
pub const FGuid = @import("core.zig").FGuid;
pub const FName = @import("core.zig").FName;
pub const FNameEntry = @import("core.zig").FNameEntry;

pub const ArchiveError = error{
    InvalidPackageFileTag,
};

pub const FObjectImport = extern struct {
    class_package: u32,
    class_name: u32,
    outer_index: u32,
    unk1: u32,
    owner_ref: i32,
    object_name: u32,
    unk2: u32,

    pub fn take(r: *std.Io.Reader) !FObjectImport {
        return try r.takeStruct(FObjectImport, .little);
    }

    pub fn takeArray(r: *std.Io.Reader, allocator: mem.Allocator) ![]FObjectImport {
        const count = try r.takeInt(u32, .little);
        const imports = try allocator.alloc(FObjectImport, count);
        errdefer imports.deinit();

        for (imports) |*import| {
            import.* = try FObjectImport.take(r);
        }
        return imports.toOwnedSlice();
    }

    pub fn write(self: FObjectImport, w: *std.io.Writer) !void {
        try w.writeStruct(self, .little);
    }

    pub fn writeArray(self: []const FObjectImport, w: *std.io.Writer) !void {
        try w.writeInt(u32, @intCast(self.len), .little);
        for (self) |import| {
            try import.write(w);
        }
    }

    pub fn format(self: FObjectImport, writer: *std.io.Writer) !void {
        try writer.print(
            \\FObjectImport:
            \\  class_package: {d}
            \\  class_name: {d}
            \\  outer_index: {d}
            \\  unk1: {d}
            \\  owner_ref: {d}
            \\  object_name: {d}
            \\  unk2: {d}
            \\
            \\
        , .{
            self.class_package,
            self.class_name,
            self.outer_index,
            self.unk1,
            self.owner_ref,
            self.object_name,
            self.unk2,
        });
    }
};

pub const FCompressedChunkInfo = extern struct {
    compressed_size: u32,
    uncompressed_size: u32,

    pub fn take(r: *std.io.Reader) !FCompressedChunkInfo {
        return try r.takeStruct(FCompressedChunkInfo, .little);
    }

    pub fn takeArray(r: *std.io.Reader, allocator: mem.Allocator) ![]FCompressedChunkInfo {
        const count = try r.takeInt(u32, .little);
        const chunks = try allocator.alloc(FCompressedChunkInfo, count);
        errdefer allocator.free(chunks);

        for (chunks) |*chunk| {
            chunk.* = try FCompressedChunkInfo.take(r);
        }
        return chunks;
    }

    pub fn write(self: FCompressedChunkInfo, w: *std.io.Writer) !void {
        try w.writeStruct(self, .little);
    }

    pub fn writeArray(self: []const FCompressedChunkInfo, w: *std.io.Writer) !void {
        try w.writeInt(u32, @intCast(self.len), .little);
        for (self) |chunk| {
            try chunk.write(w);
        }
    }

    pub fn format(self: FCompressedChunkInfo, writer: *std.io.Writer) !void {
        try writer.print(
            \\FCompressedChunkInfo:
            \\  compressed_size:   {d}
            \\  uncompressed_size: {d}
            \\
            \\
        , .{
            self.compressed_size,
            self.uncompressed_size,
        });
    }
};

pub const FCompressedChunk = extern struct {
    UncompressedOffset: u32,
    UncompressedSize: u32,
    CompressedOffset: u32,
    CompressedSize: u32,

    pub fn take(r: *std.io.Reader) !FCompressedChunk {
        return try r.takeStruct(FCompressedChunk, .little);
    }

    pub fn takeArray(r: *std.io.Reader, allocator: mem.Allocator) ![]FCompressedChunk {
        const count = try r.takeInt(u32, .little);
        const chunks = try allocator.alloc(FCompressedChunk, count);
        errdefer allocator.free(chunks);

        for (chunks) |*chunk| {
            chunk.* = try FCompressedChunk.take(r);
        }
        return chunks;
    }

    pub fn write(self: FCompressedChunk, w: *std.io.Writer) !void {
        try w.writeStruct(self, .little);
    }

    pub fn writeArray(self: []const FCompressedChunk, w: *std.io.Writer) !void {
        try w.writeInt(u32, @intCast(self.len), .little);
        for (self) |chunk| {
            try chunk.write(w);
        }
    }

    pub fn format(self: FCompressedChunk, writer: *std.io.Writer) !void {
        try writer.print(
            \\FCompressedChunk:
            \\  UncompressedOffset: {d}
            \\  UncompressedSize:   {d}
            \\  CompressedOffset:   {d}
            \\  CompressedSize:     {d}
            \\
            \\
        , .{
            self.UncompressedOffset,
            self.UncompressedSize,
            self.CompressedOffset,
            self.CompressedSize,
        });
    }
};

pub const FTextureAllocation = struct {
    size_x: u32,
    size_y: u32,
    num_mips: u32,
    tex_format: u32,
    tex_create_flags: u32,
    export_indices_count: u32,
    export_indices: [*]u32,

    pub fn take(r: *std.io.Reader, allocator: mem.Allocator) !FTextureAllocation {
        var allocation: FTextureAllocation = .{
            .size_x = try r.takeInt(u32, .little),
            .size_y = try r.takeInt(u32, .little),
            .num_mips = try r.takeInt(u32, .little),
            .tex_format = try r.takeInt(u32, .little),
            .tex_create_flags = try r.takeInt(u32, .little),
            .export_indices_count = try r.takeInt(u32, .little),
            .export_indices = &.{},
        };

        if (allocation.export_indices_count > 0) {
            const indices = try allocator.alloc(u32, allocation.export_indices_count);
            errdefer allocator.free(indices);

            for (indices) |*index| {
                index.* = try r.takeInt(u32, .little);
            }

            allocation.export_indices = indices.ptr;
        }

        return allocation;
    }

    pub fn takeArray(r: *std.io.Reader, allocator: mem.Allocator) ![]FTextureAllocation {
        const count = try r.takeInt(u32, .little);
        const allocations = try allocator.alloc(FTextureAllocation, count);
        errdefer allocator.free(allocations);

        for (allocations) |*allocation| {
            allocation.* = try FTextureAllocation.take(r, allocator);
        }

        return allocations;
    }

    pub fn write(self: FTextureAllocation, w: *std.io.Writer) !void {
        try w.writeInt(u32, self.size_x, .little);
        try w.writeInt(u32, self.size_y, .little);
        try w.writeInt(u32, self.num_mips, .little);
        try w.writeInt(u32, self.tex_format, .little);
        try w.writeInt(u32, self.tex_create_flags, .little);
        try w.writeInt(u32, self.export_indices_count, .little);

        if (self.export_indices_count > 0) {
            for (self.export_indices[0..self.export_indices_count]) |index| {
                try w.writeInt(u32, index, .little);
            }
        }
    }

    pub fn writeArray(self: []const FTextureAllocation, w: *std.io.Writer) !void {
        try w.writeInt(u32, @intCast(self.len), .little);
        for (self) |allocation| {
            try allocation.write(w);
        }
    }

    pub fn format(self: FTextureAllocation, writer: *std.io.Writer) !void {
        try writer.print(
            \\FTextureAllocation:
            \\  size_x:               {d}
            \\  size_y:               {d}
            \\  num_mips:             {d}
            \\  tex_format:           {d}
            \\  tex_create_flags:     {d}
            \\  export_indices_count: {d}
            \\
            \\
        , .{
            self.size_x,
            self.size_y,
            self.num_mips,
            self.tex_format,
            self.tex_create_flags,
            self.export_indices_count,
        });
    }
};

pub const FObjectExport = extern struct {
    class_index: i32 = 0,
    super_index: i32 = 0,
    outer_index: i32 = 0,
    object_name: u32 = 0,
    archetype_index: u32 = 0,
    archetype: u32 = 0,
    object_flags: u64 = 0,
    serial_size: u32 = 0,
    serial_offset: u32 = 0,
    export_flags: u32 = 0,
    generation_net_object_count: u32 = 0,
    generation_net_objects: [*]u32 = &.{},
    package_guid: FGuid = .{},
    package_flags: u32 = 0,

    pub fn take(r: *std.Io.Reader, allocator: mem.Allocator) !FObjectExport {
        var @"export": FObjectExport = .{
            .class_index = try r.takeInt(i32, .little),
            .super_index = try r.takeInt(i32, .little),
            .outer_index = try r.takeInt(i32, .little),
            .object_name = try r.takeInt(u32, .little),
            .archetype_index = try r.takeInt(u32, .little),
            .archetype = try r.takeInt(u32, .little),
            .object_flags = try r.takeInt(u64, .little),
            .serial_size = try r.takeInt(u32, .little),
            .serial_offset = try r.takeInt(u32, .little),
            .export_flags = try r.takeInt(u32, .little),
            .generation_net_object_count = try r.takeInt(u32, .little),
        };

        if (@"export".generation_net_object_count > 0) {
            const net_objects = try allocator.alloc(u32, @"export".generation_net_object_count);
            errdefer allocator.free(net_objects);

            for (net_objects) |*net_object| {
                net_object.* = try r.takeInt(u32, .little);
            }

            @"export".generation_net_objects = net_objects.ptr;
        }

        @"export".package_guid = try r.takeStruct(FGuid, .little);
        @"export".package_flags = try r.takeInt(u32, .little);

        return @"export";
    }

    pub fn takeArray(r: *std.Io.Reader, allocator: mem.Allocator) ![]FObjectExport {
        const count = try r.takeInt(u32, .little);
        const exports = try allocator.alloc(FObjectExport, count);
        errdefer allocator.free(exports);

        for (exports) |*@"export"| {
            @"export".* = try FObjectExport.take(r, allocator);
        }

        return exports;
    }

    pub fn write(self: FObjectExport, w: *std.io.Writer) !void {
        try w.writeInt(i32, self.class_index, .little);
        try w.writeInt(i32, self.super_index, .little);
        try w.writeInt(i32, self.outer_index, .little);
        try w.writeInt(u32, self.object_name, .little);
        try w.writeInt(u32, self.archetype_index, .little);
        try w.writeInt(u32, self.archetype, .little);
        try w.writeInt(u64, self.object_flags, .little);
        try w.writeInt(u32, self.serial_size, .little);
        try w.writeInt(u32, self.serial_offset, .little);
        try w.writeInt(u32, self.export_flags, .little);
        try w.writeInt(u32, self.generation_net_object_count, .little);
        if (self.generation_net_object_count > 0) {
            for (self.generation_net_objects[0..self.generation_net_object_count]) |net_object| {
                try w.writeInt(u32, net_object, .little);
            }
        }
        try w.writeStruct(self.package_guid, .little);
        try w.writeInt(u32, self.package_flags, .little);
    }

    pub fn writeArray(self: []const FObjectExport, w: *std.io.Writer) !void {
        try w.writeInt(u32, @intCast(self.len), .little);
        for (self) |@"export"| {
            try @"export".write(w);
        }
    }

    pub fn format(self: FObjectExport, writer: *std.io.Writer) !void {
        try writer.print(
            \\FObjectExport:
            \\  class_index: {d}
            \\  super_index: {d}
            \\  outer_index: {d}
            \\  object_name: {d}
            \\  archetype_index: {d}
            \\  archetype: {d}
            \\  object_flags: {d}
            \\  serial_size: {d}
            \\  serial_offset: {d}
            \\  export_flags: {d}
            \\  generation_net_object_count: {d}
            \\  package_guid: {d}
            \\  package_flags: {d}
            \\
            \\
        , .{
            self.class_index,
            self.super_index,
            self.outer_index,
            self.object_name,
            self.archetype_index,
            self.archetype,
            self.object_flags,
            self.serial_size,
            self.serial_offset,
            self.export_flags,
            self.generation_net_object_count,
            self.package_guid,
            self.package_flags,
        });
    }
};

pub const FGenerationInfo = extern struct {
    export_count: i32,
    name_count: i32,
    net_object_count: i32,

    pub fn take(reader: *std.Io.Reader) !FGenerationInfo {
        return try reader.takeStruct(FGenerationInfo, .little);
    }

    pub fn takeArray(reader: *std.Io.Reader, allocator: mem.Allocator) ![]FGenerationInfo {
        const count = try reader.takeInt(u32, .little);
        const generations = try allocator.alloc(FGenerationInfo, count);
        errdefer allocator.free(generations);

        for (generations) |*generation| {
            generation.* = try FGenerationInfo.take(reader);
        }

        return generations;
    }

    pub fn write(self: FGenerationInfo, w: *std.io.Writer) !void {
        try w.writeStruct(self, .little);
    }

    pub fn writeArray(self: []const FGenerationInfo, w: *std.io.Writer) !void {
        try w.writeInt(u32, @intCast(self.len), .little);
        for (self) |generation| {
            try generation.write(w);
        }
    }

    pub fn format(self: FGenerationInfo, writer: *std.io.Writer) !void {
        try writer.print(
            \\FGenerationInfo:
            \\  export_count: {d}
            \\  name_count: {d}
            \\  net_object_count: {d}
            \\
            \\
        , .{
            self.export_count,
            self.name_count,
            self.net_object_count,
        });
    }
};

pub const FPackageFileSummary = struct {
    tag: u32 = 0,
    file_version: u16 = 0,
    licensee_version: u16 = 0,
    total_header_size: u32 = 0,
    folder_name: FName = .{ .len = 0, .data = undefined },
    package_flags: EPackageFlags = .{},
    name_count: u32 = 0,
    name_offset: u32 = 0,
    export_count: u32 = 0,
    export_offset: u32 = 0,
    import_count: u32 = 0,
    import_offset: u32 = 0,
    depends_offset: u32 = 0,
    import_export_guids_offset: u32 = 0,
    import_guids_count: u32 = 0,
    export_guids_count: u32 = 0,
    thumbnail_table_offset: u32 = 0,
    guid: FGuid = .{},
    generations: []FGenerationInfo = &.{},
    engine_version: u32 = 0,
    cooker_version_upper: u16 = 0,
    cooker_version_lower: u16 = 0,
    compression_flags: ECompressionFlags = .{},
    compressed_chunks: []FCompressedChunk = &.{},
    package_source: u32 = 0,
    additional_packages: []FName = &.{},
    texture_allocations: []FTextureAllocation = &.{},

    pub fn take(r: *std.Io.Reader, allocator: mem.Allocator) !FPackageFileSummary {
        const tag = try r.takeInt(u32, .little);
        if (tag != Constants.file_tag and tag != Constants.swapped_file_tag) {
            return error.InvalidPackageFileTag;
        }

        var summary: FPackageFileSummary = .{
            .tag = tag,
            .file_version = try r.takeInt(u16, .little),
            .licensee_version = try r.takeInt(u16, .little),
            .total_header_size = try r.takeInt(u32, .little),
            .folder_name = try FName.take(r, allocator),
            .package_flags = try r.takeStruct(EPackageFlags, .little),
            .name_count = try r.takeInt(u32, .little),
            .name_offset = try r.takeInt(u32, .little),
            .export_count = try r.takeInt(u32, .little),
            .export_offset = try r.takeInt(u32, .little),
            .import_count = try r.takeInt(u32, .little),
            .import_offset = try r.takeInt(u32, .little),
            .depends_offset = try r.takeInt(u32, .little),
            .import_export_guids_offset = try r.takeInt(u32, .little),
            .import_guids_count = try r.takeInt(u32, .little),
            .export_guids_count = try r.takeInt(u32, .little),
            .thumbnail_table_offset = try r.takeInt(u32, .little),
            .guid = try r.takeStruct(FGuid, .little),
            .generations = try FGenerationInfo.takeArray(r, allocator),
            .engine_version = try r.takeInt(u32, .little),
            .cooker_version_upper = try r.takeInt(u16, .little),
            .cooker_version_lower = try r.takeInt(u16, .little),
            .compression_flags = try r.takeStruct(ECompressionFlags, .little),
            .compressed_chunks = try FCompressedChunk.takeArray(r, allocator),
            .package_source = try r.takeInt(u32, .little),
            .additional_packages = try FName.takeArray(r, allocator),
            .texture_allocations = try FTextureAllocation.takeArray(r, allocator),
        };
        summary.package_flags.filter_editor_only = true;
        return summary;
    }

    pub fn write(self: *FPackageFileSummary, w: *std.io.Writer) !void {
        try w.writeInt(u32, self.tag, .little);
        try w.writeInt(u16, self.file_version, .little);
        try w.writeInt(u16, self.licensee_version, .little);
        try w.writeInt(u32, self.total_header_size, .little);
        try self.folder_name.write(w);
        self.package_flags.filter_editor_only = false;
        try w.writeStruct(self.package_flags, .little);
        try w.writeInt(u32, self.name_count, .little);
        try w.writeInt(u32, self.name_offset, .little);
        try w.writeInt(u32, self.export_count, .little);
        try w.writeInt(u32, self.export_offset, .little);
        try w.writeInt(u32, self.import_count, .little);
        try w.writeInt(u32, self.import_offset, .little);
        try w.writeInt(u32, self.depends_offset, .little);
        try w.writeInt(u32, self.import_export_guids_offset, .little);
        try w.writeInt(u32, self.import_guids_count, .little);
        try w.writeInt(u32, self.export_guids_count, .little);
        try w.writeInt(u32, self.thumbnail_table_offset, .little);
        try w.writeStruct(self.guid, .little);
        try FGenerationInfo.writeArray(self.generations, w);
        try w.writeInt(u32, self.engine_version, .little);
        try w.writeInt(u16, self.cooker_version_upper, .little);
        try w.writeInt(u16, self.cooker_version_lower, .little);
        try w.writeInt(u32, @bitCast(self.compression_flags), .little);
        try FCompressedChunk.writeArray(self.compressed_chunks, w);
        try w.writeInt(u32, self.package_source, .little);
        try FName.writeArray(self.additional_packages, w);
        try FTextureAllocation.writeArray(self.texture_allocations, w);
    }

    pub fn format(self: FPackageFileSummary, writer: *std.io.Writer) !void {
        try writer.print(
            \\FPackageFileSummary:
            \\  tag:                        0x{X}
            \\  file_version:               {d}/{d}
            \\  total_header_size:          {d}
            \\  folder_name:                {s}
            \\  package_flags:              0x{X:0>8} ({f})
            \\  name_count:                 {d}
            \\  name_offset:                {d}
            \\  export_count:               {d}
            \\  export_offset:              {d}
            \\  import_count:               {d}
            \\  import_offset:              {d}
            \\  depends_offset:             {d}
            \\  import_export_guids_offset: {d}
            \\  import_guids_count:         {d}
            \\  export_guids_count:         {d}
            \\  thumbnail_table_offset:     {d}
            \\  guid:                       {f}
            \\  engine_version:             {d}
            \\  cooker_version:             {d}/{d}
            \\  compression_flags:          0x{X:0>8} ({f})
            \\  compressed_chunks:          {d}
            \\  package_source:             0x{X}
            \\  additional_packages_count:  {d}
            \\  texture_allocations:        {d}
            \\
            \\
        , .{
            self.tag,
            self.file_version,
            self.licensee_version,
            self.total_header_size,
            self.folder_name.toString(),
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
            self.cooker_version_upper,
            self.cooker_version_lower,
            @as(u32, @bitCast(self.compression_flags)),
            self.compression_flags,
            self.compressed_chunks.len,
            self.package_source,
            self.additional_packages.len,
            self.texture_allocations.len,
        });

        if (self.additional_packages.len > 0) {
            try writer.print("\nAdditional packages:\n", .{});
            for (self.additional_packages, 0..) |pkg, i| {
                try writer.print("  {d}: {f}\n", .{ i, pkg });
            }
        }

        try writer.print("\nGenerations:\n", .{});
        for (self.generations) |generation| {
            try writer.print("{f}", .{generation});
        }

        if (self.texture_allocations.len > 0) {
            try writer.print("\nTexture allocations:\n", .{});
            for (self.texture_allocations) |allocation| {
                try writer.print("{f}", .{allocation});
            }
        }

        if (self.compressed_chunks.len > 0) {
            try writer.print("\nCompressed chunks:\n", .{});
            for (self.compressed_chunks) |chunk| {
                try writer.print("{f}", .{chunk});
            }
        }
    }
};
