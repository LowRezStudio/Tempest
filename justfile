# Builds the project with a custom optimize level
[group("dev")]
dev optimize="Debug":
  zig build --build-file Tempest.Utils/build.zig -Doptimize={{optimize}}

# Builds the project for fast release
[group("release")]
fast:
  zig build --build-file Tempest.Utils/build.zig -Doptimize=ReleaseFast

# Builds the project for small release
[group("release")]
small:
  zig build --build-file Tempest.Utils/build.zig -Doptimize=ReleaseSmall
