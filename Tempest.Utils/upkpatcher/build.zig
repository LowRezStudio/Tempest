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

    const linux32_target = b.resolveTargetQuery(.{
        .cpu_arch = .x86,
        .os_tag = .linux,
        .abi = .gnu,
    });

    const linux64_target = b.resolveTargetQuery(.{
        .cpu_arch = .x86_64,
        .os_tag = .linux,
        .abi = .gnu,
    });

    const macos64_target = b.resolveTargetQuery(.{
        .cpu_arch = .x86_64,
        .os_tag = .macos,
        .abi = .none,
    });

    const macosarm64_target = b.resolveTargetQuery(.{
        .cpu_arch = .aarch64,
        .os_tag = .macos,
        .abi = .none,
    });

    buildForTarget(b, win32_target, optimize, "Tempest.UpkPatcher32");
    buildForTarget(b, win64_target, optimize, "Tempest.UpkPatcher64");
    buildForTarget(b, linux32_target, optimize, "Tempest.UpkPatcherLinux32");
    buildForTarget(b, linux64_target, optimize, "Tempest.UpkPatcherLinux64");
    buildForTarget(b, macos64_target, optimize, "Tempest.UpkPatcherMacos64");
    buildForTarget(b, macosarm64_target, optimize, "Tempest.UpkPatcherMacosArm64");
}

fn buildForTarget(b: *std.Build, target: std.Build.ResolvedTarget, optimize: std.builtin.OptimizeMode, name: []const u8) void {
    const exe_mod = b.createModule(.{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .optimize = optimize,
    });

    const exe = b.addExecutable(.{
        .name = name,
        .root_module = exe_mod,
    });

    const parser_dep = b.dependency("upkparser", .{
        .target = target,
        .optimize = optimize,
    });
    const parser = parser_dep.module("upkparser");

    exe.root_module.addImport("upkparser", parser);

    b.installArtifact(exe);
}
