const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    const minilzo_dep = b.dependency("minilzo", .{
        .target = target,
        .optimize = optimize,
    });

    const mod = b.addModule("Tempest_Utils", .{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .imports = &.{
            .{ .name = "minilzo", .module = minilzo_dep.module("minilzo_wrapper") },
        },
    });

    const parser_exe = b.addExecutable(.{
        .name = "upk-parser",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/upk-parser/main.zig"),
            .target = target,
            .optimize = optimize,
            .imports = &.{
                .{ .name = "Tempest_Utils", .module = mod },
            },
        }),
    });

    b.installArtifact(parser_exe);

    // tests
    const mod_tests = b.addTest(.{
        .root_module = mod,
    });
    const run_mod_tests = b.addRunArtifact(mod_tests);

    const parser_tests = b.addTest(.{
        .root_module = parser_exe.root_module,
    });
    const run_parser_tests = b.addRunArtifact(parser_tests);

    const test_step = b.step("test", "Run tests");
    test_step.dependOn(&run_mod_tests.step);
    test_step.dependOn(&run_parser_tests.step);
}
