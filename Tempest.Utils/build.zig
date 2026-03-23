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

    // Create utils module with minilzo C bindings
    const utils_mod = b.addModule("utils", .{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });

    // Add minilzo C translation
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
            .root_source_file = b.path("src/minilzo/root.zig"),
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

    // Build asmloader
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
        lib.root_module.addImport("utils", utils_mod);
        lib.root_module.addImport("memory", memory_module);
        lib.root_module.addImport("detourz", detourz);

        b.installArtifact(lib);
    }

    // Build upkpatcher
    const clap = b.dependency("clap", .{});
    const upkpatcher_exe = b.addExecutable(.{
        .name = b.fmt("upkpatcher-{s}-{s}", .{ @tagName(target.result.os.tag), @tagName(target.result.cpu.arch) }),
        .root_module = b.createModule(.{
            .target = target,
            .optimize = optimize,
            .root_source_file = b.path("src/upkpatcher/root.zig"),
            .link_libc = true,
        }),
    });

    upkpatcher_exe.root_module.addImport("utils", utils_mod);
    upkpatcher_exe.root_module.addImport("clap", clap.module("clap"));
    upkpatcher_exe.root_module.addImport("minilzo_c", minilzo_c);
    upkpatcher_exe.root_module.addCSourceFile(.{
        .file = b.path("src/minilzo/vendor/minilzo.c"),
        .flags = &.{"-O3"},
    });

    b.installArtifact(upkpatcher_exe);

    const mctsparser_exe = b.addExecutable(.{
        .name = b.fmt("mctsparser-{s}-{s}", .{ @tagName(target.result.os.tag), @tagName(target.result.cpu.arch) }),
        .root_module = b.createModule(.{
            .target = target,
            .optimize = optimize,
            .root_source_file = b.path("src/mctsparser/root.zig"),
        }),
    });
    mctsparser_exe.root_module.addImport("clap", clap.module("clap"));

    b.installArtifact(mctsparser_exe);
}
