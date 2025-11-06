const std = @import("std");
const fs = std.fs;

const UpkParser = @import("utils").upkparser;
const Archive = @import("utils").upkparser.Archive;
const Patches = @import("patches.zig");
const Clap = @import("clap");

fn generateOutputPath(a: std.mem.Allocator, path: []const u8, suffix: []const u8) ![]const u8 {
    if (std.mem.lastIndexOf(u8, path, ".")) |dot| {
        const base = path[0..dot];
        const ext = path[dot..];
        if (std.mem.endsWith(u8, base, suffix)) return path;
        return try std.fmt.allocPrint(a, "{s}{s}{s}", .{ base, suffix, ext });
    }
    return try std.fmt.allocPrint(a, "{s}{s}", .{ path, suffix });
}

fn processFile(allocator: std.mem.Allocator, filepath: []const u8) !UpkParser.Parser {
    const file = try fs.cwd().openFile(filepath, .{});
    defer file.close();

    var parser: UpkParser.Parser = try .init(allocator, file, .{});
    try parser.parse();

    if (parser.summary.compression_flags.obscured) {
        return error.Obscured;
    }

    return parser;
}

fn processSingleFile(
    arena: std.mem.Allocator,
    path: []const u8,
    suffix: []const u8,
    patch_fn: anytype,
) !void {
    var parser = try processFile(arena, path);

    const success = try patch_fn(&parser);
    if (success) {
        const out = try generateOutputPath(arena, path, suffix);
        try parser.save(out);
        std.log.info("Patched file!", .{});
    } else {
        std.log.err("Skipped file", .{});
    }
}

fn processFolder(
    arena: std.mem.Allocator,
    folder: []const u8,
    suffix: []const u8,
    patch_fn: anytype,
    all_paths: ?*std.ArrayList([]const u8),
) !void {
    var dir = try fs.cwd().openDir(folder, .{ .iterate = true });
    defer dir.close();

    var iter = dir.iterate();
    var upk_files = std.ArrayList([]const u8).empty;
    defer upk_files.deinit(arena);

    while (try iter.next()) |entry| {
        if (entry.kind != .file) continue;
        if (!std.mem.endsWith(u8, entry.name, ".upk")) continue;

        const full = try std.fs.path.join(arena, &.{ folder, entry.name });
        try upk_files.append(arena, full);
    }

    if (upk_files.items.len == 0) {
        std.log.err("No UPK files found in folder!", .{});
        return error.NoFilesFound;
    }

    // If all_paths is provided, collect all paths for batch processing
    if (all_paths) |paths_list| {
        try paths_list.appendSlice(arena, upk_files.items);
        return;
    }

    // Only process files if patch_fn is actually a function
    const PatchFnType = @TypeOf(patch_fn);
    const is_undefined = PatchFnType == @TypeOf(undefined);
    if (is_undefined) return;

    std.debug.print("\n", .{});

    var skipped: usize = 0;
    var patched: usize = 0;

    for (upk_files.items, 1..) |upk_file, i| {
        std.debug.print("\rProcessing: {d}/{d} (Patched: {d}, Skipped: {d})", .{
            i, upk_files.items.len, patched, skipped,
        });

        var parser = processFile(arena, upk_file) catch {
            skipped += 1;
            continue;
        };

        const success = patch_fn(&parser) catch |e| {
            std.debug.print("\n", .{});
            std.log.err("Failed to patch file: {}", .{e});
            skipped += 1;
            continue;
        };

        if (success) {
            const out = try generateOutputPath(arena, upk_file, suffix);
            try parser.save(out);
            patched += 1;
        } else {
            skipped += 1;
        }
    }

    std.debug.print("\n", .{});
    std.log.info("Completed: {d} patched, {d} skipped out of {d} files", .{
        patched, skipped, upk_files.items.len,
    });
}

pub fn main() !void {
    var arena_alloc = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    const arena = arena_alloc.allocator();
    defer _ = arena_alloc.deinit();

    const params = comptime Clap.parseParamsComptime(
        \\-h, --help             Display this help and exit.
        \\-s, --sfsc             Patches SeekFreeShaderCache objects.
        \\-g, --guid             Patches the GuidCache object.
        \\-f, --file <str>       Input file path.
        \\-o, --output <str>     Output file path.
        \\-d, --folder <str>     Input folder path.
        \\-S, --suffix <str>     Suffix to add before extension.
        \\-v, --verbose          Print verbose output.
        \\
    );

    var diag = Clap.Diagnostic{};
    var res = Clap.parse(Clap.Help, &params, Clap.parsers.default, .{
        .diagnostic = &diag,
        .allocator = arena,
        .assignment_separators = "=:",
    }) catch |err| {
        try diag.reportToFile(.stderr(), err);
        return err;
    };
    defer res.deinit();

    const has_file = res.args.file != null;
    const has_folder = res.args.folder != null;
    const has_sfsc = res.args.sfsc != 0;
    const has_guid = res.args.guid != 0;

    if (res.args.help != 0)
        return Clap.helpToFile(.stderr(), Clap.Help, &params, .{});

    if (res.args.verbose != 0)
        std.log.info("TODO: implement", .{});

    if (!has_file and !has_folder) {
        std.log.err("Must specify either --file or --folder", .{});
        return error.InvalidArguments;
    }

    if (has_file and has_folder) {
        std.log.err("Cannot use --file and --folder together", .{});
        return error.InvalidArguments;
    }

    if (!has_sfsc and !has_guid) {
        std.log.err("Specify at least one patch: --sfsc or --guid", .{});
        return error.InvalidArguments;
    }

    if (has_sfsc and has_guid) {
        std.log.err("Cannot use --sfsc and --guid together", .{});
        return error.InvalidArguments;
    }

    const suffix = res.args.suffix orelse "";

    if (has_sfsc) {
        std.log.info("Patching SeekFreeShaderCache...", .{});

        if (res.args.file) |path| {
            try processSingleFile(arena, path, suffix, Patches.patchSFSC);
        } else if (res.args.folder) |folder| {
            try processFolder(arena, folder, suffix, Patches.patchSFSC, null);
        }
    } else if (has_guid) {
        std.log.info("Patching GuidCache...", .{});

        if (res.args.file) |path| {
            var paths = std.ArrayList([]const u8).empty;
            defer paths.deinit(arena);
            try paths.append(arena, path);

            const success = try Patches.patchGuidCache(arena, paths.items);

            if (success) {
                std.log.info("Patched file!", .{});
            } else {
                std.log.err("Skipped file", .{});
            }
        } else if (res.args.folder) |folder| {
            var all_paths = std.ArrayList([]const u8).empty;
            defer all_paths.deinit(arena);

            try processFolder(arena, folder, suffix, undefined, &all_paths);

            std.debug.print("\n", .{});

            const success = Patches.patchGuidCache(arena, all_paths.items) catch |e| {
                std.debug.print("\n", .{});
                std.log.err("Failed to patch GuidCache: {}", .{e});
                return e;
            };

            if (success) {
                std.log.info("Completed patching GuidCache", .{});
            } else {
                std.log.err("Failed to patch GuidCache", .{});
            }
        }
    }
}
