const std = @import("std");
const mem = std.mem;
const fs = std.fs;

const ue = @import("ue.zig");

const Parser = struct {
    file: fs.File,
    allocator: mem.Allocator,
    names_table: []ue.FNameEntry = undefined,
    imports_table: []ue.FObjectImport = undefined,
    exports_table: []ue.FObjectExport = undefined,
    depends_table: [][]u8 = undefined,

    pub fn init(allocator: mem.Allocator, file: fs.File) !Parser {
        return Parser{
            .file = file,
            .allocator = allocator,
        };
    }

    pub fn parse(self: Parser) !void {
        var buffer: [32 * 1024]u8 = undefined;
        var fr = self.file.reader(&buffer);
        const r: *std.Io.Reader = &fr.interface;

        const summary: ue.FPackageFileSummary = .{
            .tag = try r.takeInt(u32, .little),
            .file_version = try r.takeInt(u16, .little),
            .licensee_version = try r.takeInt(u16, .little),
            .total_header_size = try r.takeInt(u32, .little),
            .folder_name = try ue.FName.read(r),
            .package_flags = try r.takeInt(u32, .little) & ~@intFromEnum(ue.EPackageFlags.PKG_FilterEditorOnly),
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
            .guid = try r.takeStruct(ue.FGuid, .little),
            .generations = try ue.FGenerationInfo.read(r, self.allocator),
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

        ue.EPackageFlags.print(summary.package_flags);

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

        r.seek = summary.name_offset;
        var i: u32 = 0;
        while (i < summary.name_count) : (i += 1) {
            const n = try ue.FNameEntry.read(r);
            std.log.info(
                \\
                \\ NameEntry:
                \\   name:       {s}
                \\   flags:      0x{X}
            , .{
                n.name.toString(),
                n.flags,
            });

            ue.EObjectFlags.print(n.flags);
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
