const std = @import("std");
const fs = std.fs;

const UpkParser = @import("utils").upkparser;
const Archive = @import("utils").upkparser.Archive;
const Patches = @import("patches.zig");

const Config = struct {
    patch_sfsc: bool = false,
    patch_guid: bool = false,
    input_file: ?[]const u8 = null,
    input_folder: ?[]const u8 = null,
    output_file: ?[]const u8 = null,
    suffix: []const u8 = "",
};

fn printUsage(program_name: []const u8) void {
    std.debug.print(
        \\Usage: {s} [options]
        \\
        \\Options:
        \\  -sfsc              Patch SeekFreeShaderCache
        \\  -guid              Patch GUID cache
        \\  -file <path>       Input file path
        \\  -folder <path>     Input folder path (process all .upk files)
        \\  -output <path>     Output file path (only for single file mode)
        \\  -suffix <text>     Suffix to add before extension (default: "")
        \\  -h, --help         Show this help message
        \\
        \\Examples:
        \\  {s} -sfsc -file input.upk
        \\  {s} -sfsc -guid -folder ./upk_files/ -suffix _modified
        \\  {s} -sfsc -file input.upk -output custom.upk
        \\
    , .{ program_name, program_name, program_name, program_name });
}

const ArgError = error{
    InvalidArguments,
};

fn parseArgs(arena_allocator: std.mem.Allocator) !Config {
    var args_iter = try std.process.argsWithAllocator(arena_allocator);
    defer args_iter.deinit();

    // Skip program name
    _ = args_iter.next();

    var config = Config{};

    while (args_iter.next()) |arg| {
        if (std.mem.eql(u8, arg, "-sfsc")) {
            config.patch_sfsc = true;
        } else if (std.mem.eql(u8, arg, "-guid")) {
            config.patch_guid = true;
        } else if (std.mem.eql(u8, arg, "-file")) {
            if (args_iter.next()) |path| {
                config.input_file = path;
            } else {
                std.log.err("-file requires a path argument", .{});
                return error.InvalidArguments;
            }
        } else if (std.mem.eql(u8, arg, "-folder")) {
            if (args_iter.next()) |path| {
                config.input_folder = path;
            } else {
                std.log.err("-folder requires a path argument", .{});
                return error.InvalidArguments;
            }
        } else if (std.mem.eql(u8, arg, "-output")) {
            if (args_iter.next()) |path| {
                config.output_file = path;
            } else {
                std.log.err("-output requires a path argument", .{});
                return error.InvalidArguments;
            }
        } else if (std.mem.eql(u8, arg, "-suffix")) {
            if (args_iter.next()) |suffix| {
                config.suffix = suffix;
            } else {
                std.log.err("-suffix requires a text argument", .{});
                return error.InvalidArguments;
            }
        } else if (std.mem.eql(u8, arg, "-h") or std.mem.eql(u8, arg, "--help")) {
            const args_for_name = try std.process.argsAlloc(arena_allocator);
            defer std.process.argsFree(arena_allocator, args_for_name);
            printUsage(args_for_name[0]);
            std.process.exit(0);
        } else {
            std.log.err("Unknown argument: {s}", .{arg});
            return error.InvalidArguments;
        }
    }

    return config;
}

fn generateOutputPath(a: std.mem.Allocator, path: []const u8, suffix: []const u8) ![]const u8 {
    if (std.mem.lastIndexOf(u8, path, ".")) |dot| {
        const base = path[0..dot];
        const ext = path[dot..];
        if (std.mem.endsWith(u8, base, suffix)) return path;
        return try std.fmt.allocPrint(a, "{s}{s}{s}", .{ base, suffix, ext });
    }
    return try std.fmt.allocPrint(a, "{s}{s}", .{ path, suffix });
}

fn processFile(a: std.mem.Allocator, filepath: []const u8, config: Config) !void {
    const file = try fs.cwd().openFile(filepath, .{});
    defer file.close();

    var parser = try UpkParser.Parser.init(a, file, .{});
    try parser.parse();

    if (parser.summary.compression_flags.obscured) {
        std.log.info("Skipping encrypted UPK: {s}", .{filepath});
        return;
    }

    if (config.patch_sfsc) {
        if (try Patches.patchSFSC(&parser)) {
            const out = config.output_file orelse try generateOutputPath(a, filepath, config.suffix);
            try parser.save(out);
        }
    }
}

pub fn main() !void {
    var arena_alloc = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    const arena = arena_alloc.allocator();
    defer _ = arena_alloc.deinit();

    const config = parseArgs(arena) catch |err| {
        const args = try std.process.argsAlloc(arena);
        defer std.process.argsFree(arena, args);
        printUsage(args[0]);
        return err;
    };

    if (config.input_file == null and config.input_folder == null) {
        std.log.err("Specify -file or -folder", .{});
        return error.InvalidArguments;
    }

    if (config.input_file != null and config.input_folder != null) {
        std.log.err("Cannot use -file and -folder together", .{});
        return error.InvalidArguments;
    }

    if (!config.patch_sfsc and !config.patch_guid) {
        std.log.err("Specify at least one patch: -sfsc or -guid", .{});
        return error.InvalidArguments;
    }

    if (config.input_file) |path| {
        if (config.patch_guid) {
            std.log.err("-guid cannot be used with -file. Use -folder.", .{});
            return error.InvalidArguments;
        }
        return try processFile(arena, path, config);
    }

    const folder = config.input_folder.?;
    var dir = try fs.cwd().openDir(folder, .{ .iterate = true });
    defer dir.close();

    var iter = dir.iterate();
    var upk_files = std.ArrayList([]const u8).empty;
    errdefer upk_files.deinit(arena);

    while (try iter.next()) |entry| {
        if (entry.kind != .file) continue;
        if (!std.mem.endsWith(u8, entry.name, ".upk")) continue;

        const full = try std.fmt.allocPrint(arena, "{s}/{s}", .{ folder, entry.name });
        try upk_files.append(arena, full);

        if (config.patch_guid) continue;
        try processFile(arena, full, config);
    }

    if (config.patch_guid) {
        const list = upk_files.items;
        std.log.info("Running GUID patch on {} files...", .{list.len});
        if (try Patches.patchGuidCache(arena, list)) {
            std.log.info("Patched successfully!", .{});
        } else {
            std.log.err("Patch failed!", .{});
        }
    }
}
