const std = @import("std");
const fs = std.fs;

const UpkParser = @import("root.zig").upkparser;
const Archive = @import("root.zig").upkparser.Archive;

pub fn patchSFSC(parser: *UpkParser.Parser) !bool {
    var none_name_idx: u32 = 0;
    for (parser.names_table, 0..) |name_entry, i| {
        if (std.mem.eql(u8, name_entry.name.toString(), "None\x00")) {
            none_name_idx = @intCast(i);
            break;
        }
    }

    const SFSCExport = struct {
        index: u32,
        object: Archive.FObjectExport,
    };

    var sfsc_exports = std.ArrayList(SFSCExport).empty;
    errdefer sfsc_exports.deinit(parser.allocator);

    for (parser.exports_table, 0..) |exp, i| {
        if (std.mem.eql(u8, parser.names_table[exp.object_name].name.toString(), "SeekFreeShaderCache\x00")) {
            std.log.info("Found SeekFreeShaderCache export at index {d}", .{i});
            try sfsc_exports.append(parser.allocator, .{
                .index = @intCast(i),
                .object = exp,
            });
        }
    }

    if (sfsc_exports.items.len == 0) {
        std.log.err("No SeekFreeShaderCache exports found! skipping", .{});
        return false;
    }

    // Patch the SFSC exports
    for (parser.exports_table[1..], 1..) |exp, i| {
        _ = exp;

        for (sfsc_exports.items) |sfsc_export| {
            if (i == sfsc_export.index) {
                parser.exports_table[i].class_index = 0;
                parser.exports_table[i].super_index = 0;
                parser.exports_table[i].outer_index = 0;
                parser.exports_table[i].object_name = @intCast(none_name_idx);
                parser.exports_table[i].archetype_index = 0;
                parser.exports_table[i].archetype = 0;
                parser.exports_table[i].object_flags = 0;
                parser.exports_table[i].serial_size = 0;
                parser.exports_table[i].export_flags = 0;
                parser.exports_table[i].generation_net_object_count = 0;
                parser.exports_table[i].package_guid = .{};
                parser.exports_table[i].package_flags = 0;
                break;
            }
        }
    }

    return true;
}

pub fn patchGuidCache(parser: *UpkParser.Parser) !bool {
    _ = parser;
    return false;
}

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

fn generateOutputPath(allocator: std.mem.Allocator, input_path: []const u8, suffix: []const u8) ![]const u8 {
    // Find the last dot for the extension
    if (std.mem.lastIndexOf(u8, input_path, ".")) |dot_index| {
        const base = input_path[0..dot_index];
        const ext = input_path[dot_index..];
        if (std.mem.endsWith(u8, base, suffix)) {
            return input_path;
        }

        return try std.fmt.allocPrint(allocator, "{s}{s}{s}", .{ base, suffix, ext });
    } else {
        // No extension found, just append suffix
        return try std.fmt.allocPrint(allocator, "{s}{s}", .{ input_path, suffix });
    }
}

fn processFile(allocator: std.mem.Allocator, filepath: []const u8, config: Config) !void {
    // std.log.info("Processing file: {s}", .{filepath});

    const cwd = fs.cwd();
    const file = cwd.openFile(filepath, .{}) catch |e| {
        std.log.err("Error opening file '{s}': {}", .{ filepath, e });
        return e;
    };
    defer file.close();

    var parser = try UpkParser.Parser.init(allocator, file, .{});
    try parser.parse();

    const output_path = if (config.output_file) |output|
        output
    else
        try generateOutputPath(allocator, filepath, config.suffix);

    if (config.patch_sfsc) {
        std.log.info("Applying SFSC patch...", .{});
        if (try patchSFSC(&parser)) {
            try parser.save(output_path);
            // std.log.info("Saved to: {s}", .{output_path});
        }
    }

    // only run on GuidCache.upk (case unsensitive check)
    if (config.patch_guid) {
        const filename = if (std.mem.lastIndexOf(u8, filepath, "/")) |last_slash|
            filepath[last_slash + 1 ..]
        else if (std.mem.lastIndexOf(u8, filepath, "\\")) |last_backslash|
            filepath[last_backslash + 1 ..]
        else
            filepath;

        if (std.ascii.eqlIgnoreCase(filename, "guidcache.upk")) {
            std.log.info("Applying GUID cache patch...", .{});
            if (try patchGuidCache(&parser)) {
                try parser.save(output_path);
                // std.log.info("Saved to: {s}", .{output_path});
            }
        } else {
            std.log.info("Skipping GUID cache patch (not GuidCache.upk file)", .{});
        }
    }
}

pub fn main() !void {
    var allocator = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    const arena = allocator.allocator();
    defer _ = allocator.deinit();

    const config = parseArgs(arena) catch |err| {
        if (err == error.OutOfMemory) return err;
        const args = try std.process.argsAlloc(arena);
        defer std.process.argsFree(arena, args);
        printUsage(args[0]);
        return;
    };

    if (config.input_file == null and config.input_folder == null) {
        std.log.err("Either -file or -folder must be specified", .{});
        return error.InvalidArguments;
    }

    if (config.input_file != null and config.input_folder != null) {
        std.log.err("Cannot specify both -file and -folder", .{});
        return error.InvalidArguments;
    }

    if (!config.patch_sfsc and !config.patch_guid) {
        std.log.err("At least one patch option (-sfsc or -guid) must be specified", .{});
        return error.InvalidArguments;
    }

    if (config.input_file) |filepath| {
        try processFile(arena, filepath, config);
    } else if (config.input_folder) |folder_path| {
        const cwd = fs.cwd();
        var dir = try cwd.openDir(folder_path, .{ .iterate = true });
        defer dir.close();

        var iter = dir.iterate();
        while (try iter.next()) |entry| {
            if (entry.kind != .file) continue;

            if (std.mem.endsWith(u8, entry.name, ".upk")) {
                const full_path = try std.fmt.allocPrint(arena, "{s}/{s}", .{ folder_path, entry.name });
                try processFile(arena, full_path, config);
            }
        }
    }
}
