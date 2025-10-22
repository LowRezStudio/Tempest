const std = @import("std");
const mem = std.mem;
const fs = std.fs;

const PackageFlags = enum(u32) {
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

const FGuid = extern struct {
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

const FName = extern struct {
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

const FGenerationInfo = extern struct {
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

const FPackageFileSummary = struct {
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

const Parser = struct {
    file: fs.File,
    allocator: mem.Allocator,

    pub fn init(allocator: mem.Allocator, file: fs.File) !Parser {
        return Parser{
            .file = file,
            .allocator = allocator,
        };
    }

    pub fn parse(self: Parser) !void {
        var buffer: [4096]u8 = undefined;
        var fr = self.file.reader(&buffer);
        const r: *std.Io.Reader = &fr.interface;

        const summary: FPackageFileSummary = .{
            .tag = try r.takeInt(u32, .little),
            .file_version = try r.takeInt(u16, .little),
            .licensee_version = try r.takeInt(u16, .little),
            .total_header_size = try r.takeInt(u32, .little),
            .folder_name = try FName.read(r),
            .package_flags = try r.takeInt(u32, .little) & ~@intFromEnum(PackageFlags.FilterEditorOnly),
            .name_count = try r.takeInt(u32, .little),
            .name_offset = try r.takeInt(u32, .little),
            .export_count = try r.takeInt(u32, .little),
            .export_offset = try r.takeInt(u32, .little),
            .import_count = try r.takeInt(u32, .little),
            .import_offset = try r.takeInt(u32, .little),
            .depends_offset = try r.takeInt(u32, .little),
            .unk1 = try r.takeInt(u32, .little),
            .unk2 = try r.takeInt(u32, .little),
            .unk3 = try r.takeInt(u32, .little),
            .unk4 = try r.takeInt(u32, .little),
            .guid = try r.takeStruct(FGuid, .little),
            .generations = try FGenerationInfo.read(r, self.allocator),
            .engine_version = try r.takeInt(u32, .little),
            .cooker_version_upper = try r.takeInt(u16, .little),
            .cooker_version_lower = try r.takeInt(u16, .little),
            .compression_flags = try r.takeInt(u32, .little),
        };

        defer self.allocator.free(summary.generations);

        std.log.info("Package File Summary", .{});
        std.log.info(
            \\
            \\  tag:               0x{X}
            \\  file_version:      {d}/{d}
            \\  total_header_size: {d}
            \\  folder_name:       {s}
            \\  package_flags:     0x{X}
            \\  name_count:        {d}
            \\  name_offset:       {d}
            \\  export_count:      {d}
            \\  export_offset:     {d}
            \\  import_count:      {d}
            \\  import_offset:     {d}
            \\  depends_offset:    {d}
            \\  unk1:              {d}
            \\  unk2:              {d}
            \\  unk3:              {d}
            \\  unk4:              {d}
            \\  guid:              {d}
            \\  engine_version:    {d}
            \\  cooker_version:    {d}/{d}
            \\  compression_flags: {d}
        , .{
            summary.tag,
            summary.file_version,
            summary.licensee_version,
            summary.total_header_size,
            summary.folder_name.toString(),
            summary.package_flags,
            summary.name_count,
            summary.name_offset,
            summary.export_count,
            summary.export_offset,
            summary.import_count,
            summary.import_offset,
            summary.depends_offset,
            summary.unk1,
            summary.unk2,
            summary.unk3,
            summary.unk4,
            summary.guid,
            summary.engine_version,
            summary.cooker_version_upper,
            summary.cooker_version_lower,
            summary.compression_flags,
        });

        PackageFlags.print(summary.package_flags);

        // print the generations
        std.log.info("Generations", .{});
        std.log.info("gen count: {d}", .{summary.generations.len});
        for (summary.generations) |generation| {
            std.log.info(
                \\
                \\  export_count:      {d}
                \\  name_count:        {d}
                \\  net_object_count:  {d}
            , .{
                generation.export_count,
                generation.name_count,
                generation.net_object_count,
            });
        }
    }
};

pub fn main() !void {
    var allocator = std.heap.DebugAllocator(.{}).init;
    const gpa = allocator.allocator();
    defer _ = allocator.deinit();

    const args = try std.process.argsAlloc(gpa);
    defer std.process.argsFree(gpa, args);

    if (args.len < 2) {
        std.log.err("Usage: {s} <file.upk>", .{args[0]});
        return;
    }

    const filepath = args[1];
    std.log.info("filepath: {s}", .{filepath});

    const cwd = fs.cwd();
    const file = cwd.openFile(filepath, .{}) catch |e| {
        return switch (e) {
            error.FileNotFound => std.log.err("File not found!", .{}),
            else => std.log.err("Error: {}", .{e}),
        };
    };
    errdefer file.close();

    const parser = try Parser.init(gpa, file);
    _ = try parser.parse();
}
