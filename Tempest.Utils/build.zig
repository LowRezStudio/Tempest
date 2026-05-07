const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    // Main utils module
    const utils_mod = b.addModule("utils", .{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });

    // MiniLZO C bindings
    const minilzo_c = b.addTranslateC(.{
        .target = target,
        .optimize = optimize,
        .link_libc = true,
        .root_source_file = b.path("src/minilzo/vendor/minilzo.h"),
    }).createModule();
    utils_mod.addImport("minilzo_c", minilzo_c);

    // Build minilzo
    const minilzo_lib = b.addLibrary(.{
        .name = b.fmt("minilzo-{s}-{s}", .{ @tagName(target.result.os.tag), @tagName(target.result.cpu.arch) }),
        .root_module = b.createModule(.{
            .target = target,
            .optimize = optimize,
            .root_source_file = b.path("src/minilzo/main.zig"),
            .link_libc = true,
        }),
    });

    minilzo_lib.root_module.addImport("utils", utils_mod);
    minilzo_lib.root_module.addImport("minilzo_c", minilzo_c);
    minilzo_lib.root_module.addCSourceFile(.{
        .file = b.path("src/minilzo/vendor/minilzo.c"),
        .flags = &.{"-O3"},
    });
    minilzo_lib.installHeader(b.path("src/minilzo/vendor/minilzo.h"), "minilzo.h");

    b.installArtifact(minilzo_lib);
}
