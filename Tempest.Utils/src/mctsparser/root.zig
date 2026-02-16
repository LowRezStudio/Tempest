const std = @import("std");
const fs = std.fs;

const Clap = @import("clap");

const FieldEntry = @import("tokens.zig").FieldEntry;
const Fields = @import("tokens.zig").Fields;
const FunctionDetail = @import("tokens.zig").FunctionDetail;
const Functions = @import("tokens.zig").Functions;
const Parser = @import("parser.zig").Parser;

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    const allocator = arena.allocator();
    defer _ = arena.deinit();

    const params = comptime Clap.parseParamsComptime(
        \\-h, --help             Display this help and exit.
        \\-v, --verbose          Print verbose output.
        \\-V, --version <str>    legacy or modern, defaults to Modern
        \\-f, --fields <str>     Input fields file path.
        \\-F, --functions <str>  Input functions file path.
        \\-o, --output <str>     Output folder path.
        \\-e, --encrypted        Is the file encrypted
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

    res.args.version = "modern";

    if (res.args.help != 0)
        return Clap.helpToFile(.stderr(), Clap.Help, &params, .{});

    if (res.args.verbose != 0)
        std.log.info("TODO: implement", .{});

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

    const fields_path = try fs.cwd().openFile(res.args.fields orelse return error.InvalidArguments, .{});
    defer fields_path.close();

    const functions_path = try fs.cwd().openFile(res.args.functions orelse return error.InvalidArguments, .{});
    defer functions_path.close();

    const fields = Fields.init(try FieldEntry.init(allocator, fields_path));
    const functions = try Functions.init(allocator, try FunctionDetail.init(allocator, functions_path));

    const parser = try Parser.init(.{
        .allocator = allocator,
        .fields = fields,
        .functions = functions,
        .mode = if (res.args.serialize != 0) .serialize else .deserialize,
        .version = if (std.mem.eql(u8, res.args.version.?, "legacy")) .legacy else .modern,
        .is_encrypted = res.args.encrypted != 0,
    });

    try parser.printDebug();

    if (res.args.serialize != 0) {
        try parser.serialize();
    } else {
        try parser.deserialize();
    }
}
