# Tempest.Scripting

Standalone Rust workspace for the Paladins LuaJIT modding DLL and shipped Lua
mods. Apply the [root guide](../AGENTS.md); start with the [README](README.md)
for API, runtime and platform-specific build instructions.

## Rules

- Build both Win64 and Win32 DLLs from the same source. Architecture selection
  belongs to the CLI; `dlls/64/` and `dlls/32/` target matching clients only.
- Win32 LuaJIT host tools need 32-bit pointers. The macOS MinGW/Wine build uses
  `panic = "abort"` due to SJLJ/DWARF incompatibility; do not assume Win64 unwinding.
- Shipped mods live in [mods](mods/), not staging output in `dist/`.
- ImGui is vendored under [native](crates/tempest-scripting/native/);
  regenerate bindings with [gen_imgui.py](scripts/gen_imgui.py).
- Host checks do not need a game image, but do need a C++ compiler for ImGui.
- Code-bearing mods must follow [signing/trust policy](../Tempest.CLI/docs/mods.md).
  Never commit the private signing key.

## Commands

From the repository root:

```sh
just scripting-test
just scripting-check
just scripting-build
just scripting-run <game> [args]
```

Focused host checks, from this project:

```sh
cargo test -p tempest-scripting
cargo clippy -p tempest-scripting --all-targets
```

Use the [README build section](README.md#build) for Windows MSVC packaging or
MinGW cross-compilation. [Development/runtime details](docs/development.md)
cover target selection, log locations, runtime settings and dev hot reload.
