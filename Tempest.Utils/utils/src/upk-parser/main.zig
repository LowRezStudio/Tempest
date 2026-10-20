const std = @import("std");
const builtin = @import("builtin");

const debug = @import("debug.zig");
const Parser = @import("Parser.zig");

pub fn main(init: std.process.Init) !void {
    var allocator = init.arena.allocator();
    if (builtin.mode == .Debug) {
        allocator = init.gpa;
    }

    const io = init.io;
    const args = try init.minimal.args.toSlice(init.arena.allocator());

    if (args.len < 2) {
        std.debug.print("Usage: {s} <file.upk> [-report] [-save <compressed|uncompressed> <out.upk>]\n", .{args[0]});
        return;
    }

    var p = try Parser.init(io, args[1], allocator);
    defer p.deinit();

    try p.parse();

    var save_mode: ?Parser.SaveMode = null;
    var save_path: ?[]const u8 = null;

    var i: usize = 2;
    while (i < args.len) : (i += 1) {
        if (std.mem.eql(u8, args[i], "-report")) {
            debug.generatePackageReport(&p);
        } else if (std.mem.eql(u8, args[i], "-props")) {
            if (i + 1 >= args.len) {
                std.debug.print("error: -props needs an export index\n", .{});
                return;
            }
            const index = std.fmt.parseInt(usize, args[i + 1], 10) catch {
                std.debug.print("error: -props expects a number, got '{s}'\n", .{args[i + 1]});
                return;
            };
            debug.printExportProperties(&p, index);
            i += 1;
        } else if (std.mem.eql(u8, args[i], "-save")) {
            if (i + 2 >= args.len) {
                std.debug.print("error: -save needs <compressed|uncompressed> <out.upk>\n", .{});
                return;
            }
            if (std.mem.eql(u8, args[i + 1], "compressed")) {
                save_mode = .compressed;
            } else if (std.mem.eql(u8, args[i + 1], "uncompressed")) {
                save_mode = .uncompressed;
            } else {
                std.debug.print("error: unknown -save mode '{s}' (expected 'compressed' or 'uncompressed')\n", .{args[i + 1]});
                return;
            }
            save_path = args[i + 2];
            i += 2;
        } else {
            std.debug.print("error: unknown argument '{s}'\n", .{args[i]});
            return;
        }
    }

    if (save_mode) |mode| {
        const bytes = try p.save(mode);
        defer p.allocator.free(bytes);
        try std.Io.Dir.cwd().writeFile(init.io, .{
            .sub_path = save_path.?,
            .data = bytes,
            .flags = .{ .truncate = true },
        });
        std.debug.print("saved {d} bytes ({s}) to {s}\n", .{ bytes.len, @tagName(mode), save_path.? });
    }

    std.debug.print("parsed: {d} names, {d} imports, {d} exports\n", .{
        p.name_map.len,
        p.import_map.len,
        p.export_map.len,
    });
}
