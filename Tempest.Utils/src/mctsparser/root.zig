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

    // Load fields an tokens

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

    // NOTE: Debug test
    std.debug.print("Fields: {f}\n", .{mcts.Fields});
    std.debug.print("Functions: {f}\n", .{mcts.Functions});
}
