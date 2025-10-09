const std = @import("std");

pub fn build(b: *std.Build) void {
    const optimize = b.standardOptimizeOption(.{});

    const win32_target = b.resolveTargetQuery(.{
        .cpu_arch = .x86,
        .os_tag = .windows,
        .abi = .gnu,
    });

    const win64_target = b.resolveTargetQuery(.{
        .cpu_arch = .x86_64,
        .os_tag = .windows,
        .abi = .gnu,
    });

    buildForTarget(b, win32_target, optimize, "Tempest.AsmLoader32");
    buildForTarget(b, win64_target, optimize, "Tempest.AsmLoader64");
}

fn buildForTarget(b: *std.Build, target: std.Build.ResolvedTarget, optimize: std.builtin.OptimizeMode, name: []const u8) void {
    const detourz_dep = b.dependency("detourz", .{
        .target = target,
        .optimize = optimize,
    });

    const detourz = detourz_dep.module("detourz");

    const memory_dep = b.dependency("memory", .{
        .target = target,
        .optimize = optimize,
    });

    const memory = memory_dep.module("memory");

    const lib_mod = b.createModule(.{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .optimize = optimize,
    });

    const lib = b.addLibrary(.{
        .linkage = .dynamic,
        .name = name,
        .root_module = lib_mod,
    });

    lib.root_module.addImport("detourz", detourz);
    lib.root_module.addImport("memory", memory);

    b.installArtifact(lib);
}
