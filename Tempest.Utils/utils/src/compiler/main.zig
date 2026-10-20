const std = @import("std");

const compile = @import("compile.zig");
const frontend = @import("frontend.zig");
const package = @import("package.zig");

pub fn main(init: std.process.Init) !void {
    var allocator = init.arena.allocator();
    const io = init.io;
    const args = try init.minimal.args.toSlice(allocator);

    if (args.len < 4) {
        std.debug.print(
            \\usage: compiler <PackageName> <out.upk> <Class1.uc> [Class2.uc ...]
            \\
            \\Compiles UnrealScript source files into a UE3 package (.upk).
            \\Each .uc file should declare one class.
            \\
        , .{});
        return;
    }

    const package_name = args[1];
    const out_path = args[2];
    const files = args[3..];

    var c = compile.Compiler.init(allocator, package_name);
    defer c.deinit();

    for (files) |file| {
        const src = try std.Io.Dir.cwd().readFileAlloc(io, file, allocator, .unlimited);
        const cls = frontend.compileSource(&c, src, file) catch |err| {
            if (c.err) |msg| {
                std.debug.print("compile error: {s}\n", .{msg});
            } else {
                std.debug.print("compile error ({s}): {s}\n", .{ @errorName(err), file });
            }
            return;
        };
        std.debug.print("compiled class {s} ({d} fields, {d} functions, {d} states)\n", .{
            cls.name, cls.fields.items.len, cls.functions.items.len, cls.states.items.len,
        });
    }

    // Finalize: assign remaining export indices (CDOs) and serialize.
    const bytes = package.buildPackage(&c, allocator) catch |err| {
        std.debug.print("package build error: {s}\n", .{@errorName(err)});
        return;
    };
    defer allocator.free(bytes);

    try std.Io.Dir.cwd().writeFile(io, .{
        .sub_path = out_path,
        .data = bytes,
        .flags = .{ .truncate = true },
    });

    std.debug.print("wrote {d} bytes to {s}\n", .{ bytes.len, out_path });
}
