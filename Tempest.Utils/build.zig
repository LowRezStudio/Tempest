const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    // Create utils module with minilzo C bindings
    const utils_mod = b.addModule("utils", .{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });
    utils_mod.addImport("utils", utils_mod);

    // Add minilzo C translation
    const minilzo_c = b.addTranslateC(.{
        .target = target,
        .optimize = optimize,
        .link_libc = true,
        .root_source_file = b.path("src/minilzo/vendor/minilzo.h"),
    }).addModule("minilzo_c");
    utils_mod.addImport("minilzo_c", minilzo_c);

    // Build artifacts
    buildMinilzo(b, target, optimize, utils_mod, minilzo_c);
    buildAsmloader(b, optimize, utils_mod);
    buildUpkPatcher(b, target, optimize, utils_mod);
}

fn buildMinilzo(
    b: *std.Build,
    target: std.Build.ResolvedTarget,
    optimize: std.builtin.OptimizeMode,
    utils_mod: *std.Build.Module,
    minilzo_c: *std.Build.Module,
) void {
    const lib = b.addLibrary(.{
        .name = b.fmt("minilzo-{s}-{s}", .{ @tagName(target.result.os.tag), @tagName(target.result.cpu.arch) }),
        .root_module = b.createModule(.{
            .target = target,
            .optimize = optimize,
            .root_source_file = b.path("src/minilzo/root.zig"),
            .link_libc = true,
        }),
    });

    lib.root_module.addImport("utils", utils_mod);
    lib.root_module.addImport("minilzo_c", minilzo_c);
    lib.root_module.addCSourceFile(.{
        .file = b.path("src/minilzo/vendor/minilzo.c"),
        .flags = &.{"-O3"},
    });
    lib.installHeader(b.path("src/minilzo/vendor/minilzo.h"), "minilzo.h");

    b.installArtifact(lib);
}

fn buildAsmloader(
    b: *std.Build,
    optimize: std.builtin.OptimizeMode,
    utils_mod: *std.Build.Module,
) void {
    const targets: []const std.Build.ResolvedTarget = &.{
        b.resolveTargetQuery(.{ .cpu_arch = .x86, .os_tag = .windows, .abi = .gnu }),
        b.resolveTargetQuery(.{ .cpu_arch = .x86_64, .os_tag = .windows, .abi = .gnu }),
    };

    for (targets) |target| {
        const detourz = b.dependency("detourz", .{
            .target = target,
            .optimize = optimize,
        }).module("detourz");

        const lib = b.addLibrary(.{
            .linkage = .dynamic,
            .name = b.fmt("asmloader-{s}_{s}", .{ @tagName(target.result.os.tag), @tagName(target.result.cpu.arch) }),
            .root_module = b.createModule(.{
                .target = target,
                .optimize = optimize,
                .root_source_file = b.path("src/asmloader.zig"),
                .link_libc = true,
            }),
        });

        lib.root_module.addImport("utils", utils_mod);
        lib.root_module.addImport("detourz", detourz);

        b.installArtifact(lib);
    }
}

fn buildUpkPatcher(
    b: *std.Build,
    target: std.Build.ResolvedTarget,
    optimize: std.builtin.OptimizeMode,
    utils_mod: *std.Build.Module,
) void {
    const exe = b.addExecutable(.{
        .name = b.fmt("upkpatcher-{s}-{s}", .{ @tagName(target.result.os.tag), @tagName(target.result.cpu.arch) }),
        .root_module = b.createModule(.{
            .target = target,
            .optimize = optimize,
            .root_source_file = b.path("src/upkpatcher.zig"),
            .link_libc = true,
        }),
    });

    exe.root_module.addImport("utils", utils_mod);
    exe.root_module.addCSourceFile(.{
        .file = b.path("src/minilzo/vendor/minilzo.c"),
        .flags = &.{"-O3"},
    });

    b.installArtifact(exe);
}
