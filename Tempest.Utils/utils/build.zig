const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    const minilzo_dep = b.dependency("minilzo", .{
        .target = target,
        .optimize = optimize,
    });

    const unreal_mod = b.addModule("unreal", .{
        .root_source_file = b.path("src/upk-parser/unreal.zig"),
        .target = target,
        .optimize = optimize,
    });

    const mod = b.addModule("Tempest_Utils", .{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .imports = &.{
            .{ .name = "minilzo", .module = minilzo_dep.module("minilzo_wrapper") },
            .{ .name = "unreal", .module = unreal_mod },
        },
    });

    const mod_tests = b.createModule(.{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "minilzo", .module = minilzo_dep.module("minilzo_wrapper") },
            .{ .name = "unreal", .module = unreal_mod },
        },
    });

    const compiler_exe = b.addExecutable(.{
        .name = "compiler",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/compiler/main.zig"),
            .target = target,
            .optimize = optimize,
            .imports = &.{
                .{ .name = "Tempest_Utils", .module = mod },
                .{ .name = "minilzo", .module = minilzo_dep.module("minilzo_wrapper") },
                .{ .name = "unreal", .module = unreal_mod },
            },
        }),
    });
    b.installArtifact(compiler_exe);

    const run_compiler = b.addRunArtifact(compiler_exe);
    run_compiler.step.dependOn(b.getInstallStep());
    run_compiler.addPassthruArgs();
    const compiler_step = b.step("compiler", "Run the UnrealScript compiler with a package name, output path, and .uc files");
    compiler_step.dependOn(&run_compiler.step);

    const parser_exe = b.addExecutable(.{
        .name = "upk-parser",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/upk-parser/main.zig"),
            .target = target,
            .optimize = optimize,
            .imports = &.{
                .{ .name = "Tempest_Utils", .module = mod },
                .{ .name = "minilzo", .module = minilzo_dep.module("minilzo_wrapper") },
                .{ .name = "unreal", .module = unreal_mod },
            },
        }),
    });

    b.installArtifact(parser_exe);

    const run_cmd = b.addRunArtifact(parser_exe);
    run_cmd.step.dependOn(b.getInstallStep());
    run_cmd.addPassthruArgs();
    const run_step = b.step("run", "Run the upk-parser with a file path");
    run_step.dependOn(&run_cmd.step);

    // Module unit tests
    const mod_test_run = b.addTest(.{ .root_module = mod_tests });
    const run_mod_tests = b.addRunArtifact(mod_test_run);

    const test_step = b.step("test", "Run all tests");
    test_step.dependOn(&run_mod_tests.step);
}
