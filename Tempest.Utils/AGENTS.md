# Tempest.Utils

Zig 0.15.2 package producing the `asmloader` DLLs injected by the CLI, for both
Windows client architectures. Apply the [root guide](../AGENTS.md).

## Build (repository root)

```sh
just utils
just utils ReleaseFast
just utils-release
```

Or run `zig build -Doptimize=Debug` from this directory.

- Always build from the package directory. Zig's external `--build-file` form
  breaks the relative cache.
- ReleaseSmall is the launcher bundle's optimization mode.
- Output in `zig-out/bin/` is generated, not source.
- On macOS 26 with Zig < 0.16, use the existing `zign` wrapper detection or `ZIG`
  override rather than introducing a different build path.
- Keep both architecture outputs compatible with the CLI and launcher bundle.

See [build/artifact details](docs/development.md) and
[sidecar CI](../Tempest.Launcher/docs/releases.md) when changing packaging.
