const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{
        .default_target = .{
            .cpu_arch = .x86,
            .os_tag = .windows,
            .abi = .gnu,
        },
    });
    const optimize = b.standardOptimizeOption(.{});

    const minilzo_translatec = b.addTranslateC(.{
        .target = target,
        .optimize = optimize,
        .link_libc = true,
        .root_source_file = b.path("vendor/minilzo.h"),
    });

    const minilzo_c_mod = minilzo_translatec.addModule("minilzo_c");

    const lib_mod = b.addModule("minilzo", .{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });

    const lib = b.addLibrary(.{
        .linkage = .dynamic,
        .name = "minilzo",
        .root_module = lib_mod,
    });

    lib.root_module.addImport("minilzo_c", minilzo_c_mod);
    lib.root_module.addCSourceFile(.{
        .file = b.path("vendor/minilzo.c"),
        .flags = &.{"-O3"},
    });

    lib.installHeader(b.path("vendor/minilzo.h"), "minilzo.h");

    b.installArtifact(lib);
}
