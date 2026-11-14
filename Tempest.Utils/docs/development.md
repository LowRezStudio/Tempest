# Native injection utilities

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

- `Tempest.Utils/` — Zig package (Zig 0.15.2): the `asmloader` DLL the CLI injects, cross-compiled
  for both Windows clients, and the [`faststart`](faststart.md) DLL (8.1 Win64 client only)

## Zig / native utils commands

```bash
just utils                 # Debug
just utils ReleaseFast     # any -Doptimize value
just utils-release         # ReleaseSmall (what the launcher bundle ships)
# or explicitly, from each package directory (build from cwd, not --build-file:
# zig's --build-file-from-outside form corrupts the relative cache)
cd Tempest.Utils && zig build -Doptimize=Debug
```

Artifacts land in `Tempest.Utils/zig-out/bin/` (asmloader, faststart); the launcher bundle consumes the
`asmloader-windows_*.dll` and `faststart-windows_x86_64.dll` outputs (`scripts/build-cli.js` builds it). On macOS 26 with zig <0.16 the
`zign` wrapper is needed; the justfile and build-cli.js pick it up automatically when it is on
`PATH` (`ZIG=... just utils` overrides).
