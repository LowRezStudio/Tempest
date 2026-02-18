const std = @import("std");
const fs = std.fs;

const Clap = @import("clap");

const mcts = @import("mcts.zig");
const Parser = @import("parser.zig").Parser;

pub fn main() !void {
    var gpa: std.heap.DebugAllocator(.{}) = .init;
    const allocator = gpa.allocator();
    defer _ = gpa.deinit();

    const params = comptime Clap.parseParamsComptime(
        \\-h, --help             Display this help and exit.
        \\-v, --verbose          Print verbose output.
        \\-l, --legacy           Legacy version mode.
        \\-f, --fields <str>     Input fields file path.
        \\-F, --functions <str>  Input functions file path.
        \\-i, --input <str>      Input file path.
        \\-o, --output <str>     Output folder path.
        \\-e, --obscure          Is the file encrypted
        \\-s, --serialize        Serialize to MCTS
        \\-d, --deserialize      Deserialize to JSON
    );

    var diag = Clap.Diagnostic{};
    var res = Clap.parse(Clap.Help, &params, Clap.parsers.default, .{
        .diagnostic = &diag,
        .allocator = allocator,
        .assignment_separators = "=:",
    }) catch |err| {
        try diag.reportToFile(.stderr(), err);
        return err;
    };
    defer res.deinit();

    // TODO: help by default if no args provided

    if (res.args.help != 0)
        return Clap.helpToFile(.stderr(), Clap.Help, &params, .{});

    if (res.args.verbose != 0)
        std.log.info("TODO: implement", .{});

    if (res.args.input == null) {
        std.log.err("Must specify --input", .{});
        return error.InvalidArguments;
    }

    if (res.args.fields == null or res.args.functions == null) {
        std.log.err("Must specify both --fields and --functions", .{});
        return error.InvalidArguments;
    }

    if (res.args.serialize == 0 and res.args.deserialize == 0) {
        std.log.err("Must specify either --serialize or --deserialize", .{});
        return error.InvalidArguments;
    } else if (res.args.serialize != 0 and res.args.deserialize != 0) {
        std.log.err("Cannot use --serialize and --deserialize together", .{});
        return error.InvalidArguments;
    }

    if (res.args.legacy != 0) {
        std.log.err("Legacy mode is not yet implemented", .{});
        return error.InvalidArguments;
    }

    // Load fields and functions
    const fields_file = try fs.cwd().openFile(res.args.fields.?, .{});
    defer fields_file.close();

    const functions_file = try fs.cwd().openFile(res.args.functions.?, .{});
    defer functions_file.close();

    mcts.Fields.init(allocator, fields_file) catch |err| {
        std.log.err("Failed to init fields: {s}", .{@errorName(err)});
        return err;
    };
    defer mcts.Fields.deinit();

    mcts.Functions.init(allocator, functions_file) catch |err| {
        std.log.err("Failed to init functions: {s}", .{@errorName(err)});
        return err;
    };
    defer mcts.Functions.deinit();

    // NOTE: testing stuff
    std.debug.print("Fields: {f}\n", .{mcts.Fields});
    std.debug.print("Functions: {f}\n", .{mcts.Functions});

    // Create a package
    var package = try mcts.CPackage.init(allocator);
    defer package.deinit(allocator);

    // Read from file
    const read_size = try package.readFromFile(allocator, res.args.input.?, res.args.obscure != 0);
    std.debug.print("Read {d} bytes\n", .{read_size});

    // Write to file (non-obscured)
    const write_size = try package.writeToFile("./output.bin", false);
    std.debug.print("Wrote {d} bytes\n", .{write_size});

    // Set place and skip pkg flag
    const place = try package.setPlace(allocator, 1);
    std.debug.print("Set place to {d}\n", .{place});

    // Read from buffer
    var test_read_buffer: [0x7fe]u8 = undefined;
    const bytes_read = package.read(&test_read_buffer, 0x4 * 4);
    if (bytes_read == 0) return error.ReadFailed;

    std.debug.print("Read {d} bytes\n", .{bytes_read});
    for (test_read_buffer[0..bytes_read]) |b| {
        std.debug.print("{X:0>2} ", .{b});
    }
    std.debug.print("\n", .{});

    const function_id = std.mem.readInt(u32, test_read_buffer[0..4], .little);
    std.debug.print("Function: {s}\n", .{mcts.Functions.get(function_id).?.name});
    std.debug.print("Function: {X}\n", .{mcts.Functions.getByName("HELLO").?.index});

    // Test marshal
    var marshal = mcts.CMarshal.init(0);
    defer marshal.deinit(allocator);

    _ = try package.setPlace(allocator, 0);
    const success = marshal.load(allocator, &package);

    if (success) {
        std.debug.print("Function: {s}\n", .{mcts.Functions.get(marshal.function_id).?.name});
    } else {
        std.debug.print("Failed to load marshal\n", .{});
    }

    // debug print what's in the marshal
    {
        const marshal_row = marshal.base;

        std.debug.print("CMarshalRow dump:\n", .{});
        std.debug.print("  entry_count: {d}\n", .{marshal_row.entry_count});
        std.debug.print("  entry_list: 0x{x}\n", .{@intFromPtr(marshal_row.entry_list)});
        std.debug.print("  entry_tail: 0x{x}\n", .{@intFromPtr(marshal_row.entry_tail)});
        std.debug.print("  entry_pool: 0x{x}\n", .{@intFromPtr(marshal_row.entry_pool)});

        var current = marshal_row.entry_list;
        while (current) |entry| : (current = entry.next) {
            std.debug.print("  entry: 0x{x}\n", .{@intFromPtr(entry)});
        }
    }
}
