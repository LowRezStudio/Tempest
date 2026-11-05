const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    const memory_module = b.addModule("memory", .{
        .root_source_file = b.path("src/memory/root.zig"),
        .target = target,
        .optimize = optimize,
    });

    // Zydis
    const zydis = zydis: {
        const dep = b.dependency("zydis", .{});
        const zydis_lib_c = b.addTranslateC(.{
            .root_source_file = dep.path("Zydis.h"),
            .target = target,
            .optimize = optimize,
        });
        const zydis_lib = b.addLibrary(.{
            .name = "zydis",
            .linkage = .static,
            .root_module = zydis_lib_c.createModule(),
        });
        zydis_lib.root_module.addCSourceFile(.{
            .file = dep.path("Zydis.c"),
        });
        zydis_lib.root_module.addIncludePath(dep.path(""));
        break :zydis zydis_lib.root_module;
    };

    memory_module.addImport("zydis", zydis);

    // asmloader: the DLL the launcher injects into the game (both clients).
    const asmloader_targets: []const std.Build.ResolvedTarget = &.{
        b.resolveTargetQuery(.{ .cpu_arch = .x86, .os_tag = .windows, .abi = .gnu }),
        b.resolveTargetQuery(.{ .cpu_arch = .x86_64, .os_tag = .windows, .abi = .gnu }),
    };

    for (asmloader_targets) |t| {
        const detourz = b.dependency("detourz", .{
            .target = t,
            .optimize = optimize,
        }).module("detourz");

        const lib = b.addLibrary(.{
            .linkage = .dynamic,
            .name = b.fmt("asmloader-{s}_{s}", .{ @tagName(t.result.os.tag), @tagName(t.result.cpu.arch) }),
            .root_module = b.createModule(.{
                .target = t,
                .optimize = optimize,
                .root_source_file = b.path("src/asmloader.zig"),
                .link_libc = true,
            }),
        });

        lib.root_module.addImport("zydis", zydis);
        lib.root_module.addImport("memory", memory_module);
        lib.root_module.addImport("detourz", detourz);

        b.installArtifact(lib);
    }
}
