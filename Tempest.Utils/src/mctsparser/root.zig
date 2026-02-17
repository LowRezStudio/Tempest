const std = @import("std");
const fs = std.fs;

const Clap = @import("clap");

const FieldEntry = @import("tokens.zig").FieldEntry;
const Fields = @import("tokens.zig").Fields;
const FunctionDetail = @import("tokens.zig").FunctionDetail;
const Functions = @import("tokens.zig").Functions;
const Tokens = @import("tokens.zig").Tokens;
const Parser = @import("parser.zig").Parser;

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    const allocator = arena.allocator();
    defer _ = arena.deinit();

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

    const fields_path = try fs.cwd().openFile(res.args.fields orelse return error.InvalidArguments, .{});
    defer fields_path.close();

    const functions_path = try fs.cwd().openFile(res.args.functions orelse return error.InvalidArguments, .{});
    defer functions_path.close();

    // init tokens global lists
    try Tokens.init(
        allocator,
        try FunctionDetail.init(allocator, functions_path),
        try FieldEntry.init(allocator, fields_path),
    );

    const parser = try Parser.init(allocator, .{
        .file_path = res.args.input.?,
        .mode = if (res.args.serialize != 0) .serialize else .deserialize,
        .version = if (res.args.legacy == 0) .modern else .legacy,
        .obscure = res.args.obscure != 0,
    });

    // try parser.printDebug();

    if (res.args.serialize != 0) {
        try parser.serialize();
    } else {
        try parser.deserialize();
    }
}
