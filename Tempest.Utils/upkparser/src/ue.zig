const std = @import("std");
const mem = std.mem;

pub const PackageFlags = enum(u32) {
    AllowDownload = 0x00000001,
    ClientOptional = 0x00000002,
    ServerSideOnly = 0x00000004,
    Cooked = 0x00000008,
    Unsecure = 0x00000010,
    SavedWithNewerVersion = 0x00000020,
    Need = 0x00008000,
    Compiling = 0x00010000,
    ContainsMap = 0x00020000,
    Trash = 0x00040000,
    DisallowLazyLoading = 0x00080000,
    PlayInEditor = 0x00100000,
    ContainsScript = 0x00200000,
    ContainsDebugInfo = 0x00400000,
    RequireImportsAlreadyLoaded = 0x00800000,
    StoreCompressed = 0x02000000,
    StoreFullyCompressed = 0x04000000,
    ContainsFaceFXData = 0x10000000,
    NoExportAllowed = 0x20000000,
    StrippedSource = 0x40000000,
    FilterEditorOnly = 0x80000000,

    pub fn print(flags: u32) void {
        std.debug.print("Package Flags:\n", .{});
        inline for (@typeInfo(PackageFlags).@"enum".fields) |field| {
            if (flags & field.value != 0) {
                std.debug.print("  - {s}\n", .{field.name});
            }
        }
    }
};

pub const FGuid = extern struct {
    a: u32,
    b: u32,
    c: u32,
    d: u32,

    pub fn formatNumber(self: FGuid, writer: *std.Io.Writer, number: std.fmt.Number) !void {
        _ = number;
        try writer.print("{X}-{X}-{X}-{X}", .{
            self.a,
            self.b,
            self.c,
            self.d,
        });
    }
};

pub const FName = extern struct {
    len: u32,
    name: [*:0]u8,

    pub fn read(reader: *std.Io.Reader) !FName {
        const len: u32 = try reader.takeInt(u32, .little);
        const name = try reader.take(@intCast(len));
        return FName{ .len = len, .name = @ptrCast(name.ptr) };
    }

    pub fn toString(self: FName) []const u8 {
        return self.name[0..self.len];
    }
};

pub const FGenerationInfo = extern struct {
    export_count: i32,
    name_count: i32,
    net_object_count: i32,

    pub fn read(reader: *std.Io.Reader, allocator: mem.Allocator) ![]FGenerationInfo {
        const count = try reader.takeInt(u32, .little);
        const generations = try allocator.alloc(FGenerationInfo, count);

        for (generations) |*generation| {
            generation.* = FGenerationInfo{
                .export_count = try reader.takeInt(i32, .little),
                .name_count = try reader.takeInt(i32, .little),
                .net_object_count = try reader.takeInt(i32, .little),
            };
        }
        return generations;
    }
};

pub const FPackageFileSummary = struct {
    tag: u32,
    file_version: u16,
    licensee_version: u16,
    total_header_size: u32,
    folder_name: FName,
    package_flags: u32,
    name_count: u32,
    name_offset: u32,
    export_count: u32,
    export_offset: u32,
    import_count: u32,
    import_offset: u32,
    depends_offset: u32,
    unk1: u32,
    unk2: u32,
    unk3: u32,
    unk4: u32,
    guid: FGuid,
    generations: []FGenerationInfo,
    engine_version: u32,
    cooker_version_upper: u16,
    cooker_version_lower: u16,
    compression_flags: u32,
};
