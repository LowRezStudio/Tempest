# Tempest task runner. `just` lists everything; recipes are grouped per project.
#
# Override tools via env: ZIG=zign just utils   (zig <0.16 on macOS 26 needs the zign wrapper)

zig_detected := `command -v zign >/dev/null 2>&1 && echo zign || echo zig`
zig := env("ZIG", zig_detected)
scripting := "Tempest.Scripting"
unrealscript := "Tempest.UnrealScript"
launcher := "Tempest.Launcher"

default:
    @just --list --unsorted

# ---------------------------------------------------------------------------
# Tempest.Utils (Zig): asmloader, upkpatcher, mctsparser -> Tempest.Utils/zig-out/bin
# ---------------------------------------------------------------------------

# Build the native utils (Debug by default; ReleaseFast / ReleaseSmall / ReleaseSafe)
[group("utils")]
utils optimize="Debug":
    {{zig}} build --build-file Tempest.Utils/build.zig -Doptimize={{optimize}}

# Release utils, size-optimised (what the launcher bundle ships)
[group("utils")]
utils-release: (utils "ReleaseSmall")

# ---------------------------------------------------------------------------
# Tempest.Scripting (Rust): Lua modding DLL for the Paladins 8.1 Win64 client
# ---------------------------------------------------------------------------

# Host tests (no game image needed)
[group("scripting")]
scripting-test:
    cd {{scripting}} && cargo test -p tempest-scripting

# Clippy for the host and the Win64 target
[group("scripting")]
scripting-check:
    cd {{scripting}} && cargo clippy -p tempest-scripting --all-targets
    cd {{scripting}} && cargo clippy -p tempest-scripting --target x86_64-pc-windows-gnu

# Build Tempest.Scripting64.dll and stage dist/: scripting / console / dev / lib .tempest packages
[group("scripting")]
scripting-build:
    {{scripting}}/scripts/package.sh

# Launch the game with the freshly built DLL injected directly (no mod install)
[group("scripting")]
scripting-run game *args: scripting-build
    dotnet run --project Tempest.CLI -- launch "{{game}}" --dll "{{justfile_directory()}}/{{scripting}}/dist/scripting/dlls/Tempest.Scripting64.dll" {{args}}

# Install all four packages into a game folder (runtime, console, dev scratch mod, example lib)
[group("scripting")]
scripting-install game: scripting-build
    for p in scripting console lib dev; do dotnet run --project Tempest.CLI -- mod install "{{game}}" "{{scripting}}/dist/$p.tempest" --replace --allow-unsigned; done

# Repack and reinstall only the dev scratch mod, mods/dev (no Rust build)
[group("scripting")]
scripting-dev game:
    rm -rf {{scripting}}/dist/dev && cp -R {{scripting}}/mods/dev {{scripting}}/dist/dev
    mkdir -p {{scripting}}/dist/dev/types && cp {{scripting}}/types/tempest.d.lua {{scripting}}/dist/dev/types/
    cp {{scripting}}/dist/scripting/.luarc.json {{scripting}}/dist/dev/.luarc.json
    python3 {{scripting}}/scripts/pack.py {{scripting}}/dist/dev {{scripting}}/dist/dev.tempest
    dotnet run --project Tempest.CLI -- mod install "{{game}}" "{{scripting}}/dist/dev.tempest" --replace --allow-unsigned

# ---------------------------------------------------------------------------
# Tempest.UnrealScript (Rust UnrealScript compiler / decompiler, CLI `uscript`)
# ---------------------------------------------------------------------------

# Release build of the `uscript` CLI (target/release/uscript)
[group("unrealscript")]
unrealscript-build:
    cd {{unrealscript}} && cargo build --release

# Unit tests + clippy
[group("unrealscript")]
unrealscript-check:
    cd {{unrealscript}} && cargo test --release
    cd {{unrealscript}} && cargo clippy --release --all-targets

# Round-trip every script package of a cooked game dir and compare with the originals
[group("unrealscript")]
unrealscript-verify cooked:
    {{unrealscript}}/scripts/verify.sh "{{cooked}}"

# Run the CLI: just uscript info <pkg>, just uscript project build <dir> ...
[group("unrealscript")]
uscript *args: unrealscript-build
    {{unrealscript}}/target/release/uscript {{args}}

# ---------------------------------------------------------------------------
# .NET: Tempest.CLI, Tempest.Services, MarshalLib, Tempest.Protocol
# ---------------------------------------------------------------------------

# Build the whole solution (also compiles Tempest.CLI/inject via zig)
[group("dotnet")]
build:
    dotnet build Tempest.slnx

# Run the CLI, e.g. `just cli mod list <game>` / `just cli launch <game>`
[group("dotnet")]
cli *args:
    dotnet run --project Tempest.CLI -- {{args}}

# AOT publish of the CLI -> Tempest.CLI/bin/Release/net10.0/publish/
[group("dotnet")]
publish-cli:
    dotnet publish -c Release Tempest.CLI

# Run the gRPC services server
[group("dotnet")]
services:
    dotnet run --project Tempest.Services

# ---------------------------------------------------------------------------
# Tempest.Launcher (Tauri / Electron + SvelteKit)
# ---------------------------------------------------------------------------

# pnpm install (enables corepack first)
[group("launcher")]
launcher-install:
    corepack enable
    cd {{launcher}} && pnpm install

# Tauri dev app (needs the CLI built first)
[group("launcher")]
launcher-dev: build
    cd {{launcher}} && pnpm tauri dev

# Electron dev app (macOS / Linux)
[group("launcher")]
launcher-electron-dev: build
    cd {{launcher}} && pnpm electron:dev

# Production Tauri build -> src-tauri/target/release/bundle/
[group("launcher")]
launcher-build:
    cd {{launcher}} && pnpm tauri build

# svelte-check + oxlint
[group("launcher")]
launcher-check:
    cd {{launcher}} && pnpm check
    cd {{launcher}} && pnpm lint

# oxfmt
[group("launcher")]
launcher-fmt:
    cd {{launcher}} && pnpm fmt

# Regenerate TS protobuf clients from Tempest.Protocol
[group("launcher")]
launcher-proto: build
    cd {{launcher}} && pnpm proto-generate

# Version bump + annotated tag (prompts; see AGENTS.md "Releasing")
[group("launcher")]
bump:
    cd {{launcher}} && pnpm bump

# ---------------------------------------------------------------------------
# Everything
# ---------------------------------------------------------------------------

# All verification that runs without a game: .NET build, scripting tests + clippy, launcher check
[group("all")]
check: build scripting-test scripting-check unrealscript-check launcher-check
