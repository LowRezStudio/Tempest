const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    const Translator = @import("translate_c").Translator;
    const translate_c = b.dependency("translate_c", .{
        .optimize = .ReleaseFast,
    });

    const t: Translator = .init(translate_c, .{
        .c_source_file = b.path("src/vendor/minilzo.h"),
        .target = target,
        .optimize = optimize,
    });

    const minilzo = b.addModule("minilzo", .{
        .root_source_file = t.output_file,
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });
    minilzo.addImport("c_builtins", translate_c.module("c_builtins"));
    minilzo.addImport("helpers", translate_c.module("helpers"));

    const impl = b.addModule("impl", .{
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });
    impl.addCSourceFile(.{
        .file = b.path("src/vendor/minilzo.c"),
        .flags = &.{"-O3"},
    });
    // impl.sanitize_c = .off;

    const lib = b.addLibrary(.{
        .name = "minilzo",
        .linkage = .static,
        .root_module = impl,
    });
    lib.installHeader(b.path("src/vendor/minilzo.c"), "src/vendor/minilzo.h");
    b.installArtifact(lib);

    // Wrapper module for minilzo
    const wrapper = b.addModule("minilzo_wrapper", .{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
    });
    wrapper.addImport("minilzo", minilzo);
    wrapper.linkLibrary(lib);
}
