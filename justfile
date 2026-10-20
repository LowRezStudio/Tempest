# Tempest task runner. `just` lists everything; recipes are grouped per project.
#
# Override tools via env: ZIG=zign just utils   (zig <0.16 on macOS 26 needs the zign wrapper)

zig_detected := `command -v zign >/dev/null 2>&1 && echo zign || echo zig`
zig := env("ZIG", zig_detected)
scripting := "Tempest.Scripting"
unrealscript := "Tempest.UnrealScript"
launcher := "Tempest.Launcher"
# `dotnet run` of the CLI; the Web SDK would run it from Tempest.CLI/, so relative paths resolve from where just was invoked
tempest := 'dotnet run --project "' + justfile_directory() / "Tempest.CLI" + '" -p:RunWorkingDirectory="' + invocation_directory() + '"'
# R2 folder behind https://cdn.lowrezstudio.com/keys/ (rclone remote:bucket/path)
r2_keys := env("TEMPEST_R2_KEYS", "lowrezstudio:lowrezstudio-cdn/keys")

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
# Tempest.Scripting (Rust): Lua modding DLL for the Paladins 8.1 Win64 and Win32 clients
# ---------------------------------------------------------------------------

# Host tests (no game image needed)
[group("scripting")]
scripting-test:
    cd {{scripting}} && cargo test -p tempest-scripting

# Clippy for the host and both Windows targets (the i686 one builds LuaJIT's host tools under wine)
[group("scripting")]
scripting-check:
    cd {{scripting}} && cargo clippy -p tempest-scripting --all-targets
    cd {{scripting}} && cargo clippy -p tempest-scripting --target x86_64-pc-windows-gnu
    cd {{scripting}} && HOST_CC="{{justfile_directory()}}/{{scripting}}/scripts/luajit-hostcc32.sh" cargo clippy -p tempest-scripting --target i686-pc-windows-gnu

# Build Tempest.Scripting64.dll + Tempest.Scripting32.dll and stage dist/: scripting / console / dev / lib .tempest packages,
# signed with the mod signing key when there is one (TEMPEST_SCRIPTING_TARGETS="64" or "32" builds one client only)
[group("scripting")]
scripting-build:
    {{scripting}}/scripts/package.sh

# Launch the game with the freshly built DLLs injected directly (no mod install);
# the CLI injects the one matching the executable (pass --platform Win32 for the 32-bit client)
[group("scripting")]
scripting-run game *args: scripting-build
    dotnet run --project Tempest.CLI -- launch "{{game}}" --dll "{{justfile_directory()}}/{{scripting}}/dist/scripting/dlls/64/Tempest.Scripting64.dll" --dll "{{justfile_directory()}}/{{scripting}}/dist/scripting/dlls/32/Tempest.Scripting32.dll" {{args}}



# ---------------------------------------------------------------------------
# Mod signing (Tempest CLI). Mods with code (DLLs, Lua) must be signed; only keys from keys/trusted.keys
# (published to the CDN, built into the CLI as fallback) install without a warning. The signing key is
# $TEMPEST_SIGNING_KEY, else <app data>/com.lowrezstudio.tempest/signing/tempest.key; scripting-build signs with it too.
# ---------------------------------------------------------------------------

# Generate the signing keypair (never overwrites); add the .pub to keys/trusted.keys and `just keys-publish` to trust it everywhere
[group("mods")]
mod-keygen id="tempest@lowrez.studio":
    {{tempest}} -- mod gen-key --id "{{id}}"

# Package a mod folder (manifest.toml at its root) into a signed .tempest
[group("mods")]
mod-pack dir out:
    {{tempest}} -- mod pack "{{dir}}" "{{out}}"
    {{tempest}} --no-build -- mod sign "{{out}}"

# Sign a text with the mod signing key and print the armored signature (mods.lowrezstudio.com key verification)
[group("mods")]
mod-sign-text text:
    {{tempest}} -- mod sign-text "{{text}}"

# Sign existing .tempest packages in place (rewrites CHECKSUMS + CHECKSUMS.asc)
[group("mods")]
[positional-arguments]
mod-sign +packages:
    #!/usr/bin/env bash
    set -euo pipefail
    dotnet build Tempest.CLI -v q -nologo
    for pkg in "$@"; do {{tempest}} --no-build -- mod sign "$pkg"; done

# Report who signed a .tempest and whether this CLI trusts them (exit 1 unless trusted)
[group("mods")]
mod-verify package:
    {{tempest}} -- mod verify "{{package}}"

# Sign keys/trusted.keys + keys/untrusted.keys after editing them (needs a list-signing key, see Tempest.CLI/Mods/TrustStore.cs)
[group("mods")]
keys-sign:
    {{tempest}} -- mod sign-key-lists "{{justfile_directory()}}/keys"

# Sign the key lists and upload them to cdn.lowrezstudio.com/keys/ (rclone, R2), then show what the CLI now trusts
[group("mods")]
keys-publish: keys-sign
    rclone copy "{{justfile_directory()}}/keys" "{{r2_keys}}" --include "*.keys" --include "*.keys.asc" --header-upload "Cache-Control: public, max-age=300"
    {{tempest}} --no-build -- mod key-lists

# Where the key lists are fetched from and what is trusted / revoked right now
[group("mods")]
keys-status:
    {{tempest}} -- mod key-lists

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
    {{tempest}} -- {{args}}

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

# Electron dev app (needs the CLI built first)
[group("launcher")]
launcher-dev: build
    cd {{launcher}} && pnpm electron:dev

# Production Electron build -> Tempest.Launcher/dist/
[group("launcher")]
launcher-build:
    cd {{launcher}} && pnpm electron:build

# svelte-check (renderer), electron typecheck (host), oxlint
[group("launcher")]
launcher-check:
    cd {{launcher}} && pnpm check
    cd {{launcher}} && pnpm check:electron
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
