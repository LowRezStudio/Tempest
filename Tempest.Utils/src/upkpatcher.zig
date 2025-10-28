const std = @import("std");
const fs = std.fs;

const upkparser = @import("utils").upkparser;

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

    var parser = try upkparser.Parser.init(gpa, file);
    defer parser.deinit();

    try parser.parse();
}
