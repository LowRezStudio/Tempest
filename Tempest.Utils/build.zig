const std = @import("std");

pub fn build(b: *std.Build) void {
    const optimize = b.standardOptimizeOption(.{});

    // faststart: startup-time fixes for the 8.1 Win64 client, injected by the CLI.
    const t = b.resolveTargetQuery(.{ .cpu_arch = .x86_64, .os_tag = .windows, .abi = .gnu });
    const detourz = b.dependency("detourz", .{
        .target = t,
        .optimize = optimize,
    }).module("detourz");

    const lib = b.addLibrary(.{
        .linkage = .dynamic,
        .name = "faststart-windows_x86_64",
        .root_module = b.createModule(.{
            .target = t,
            .optimize = optimize,
            .root_source_file = b.path("src/faststart.zig"),
            .link_libc = true,
        }),
    });

    lib.root_module.addImport("detourz", detourz);

    b.installArtifact(lib);
}
