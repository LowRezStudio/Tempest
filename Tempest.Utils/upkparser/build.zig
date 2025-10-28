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

    const lib_mod = b.addModule("upkparser", .{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .optimize = optimize,
    });

    const lib = b.addLibrary(.{
        .linkage = .dynamic,
        .name = "upkparser",
        .root_module = lib_mod,
    });

    const minilzo_dep = b.dependency("minilzo", .{
        .target = target,
        .optimize = optimize,
    });
    const parser = minilzo_dep.module("minilzo");

    lib.root_module.addImport("minilzo", parser);

    b.installArtifact(lib);
}
