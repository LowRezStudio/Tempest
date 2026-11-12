# Tempest task runner. `just` lists everything; recipes are grouped per project.

scripting := "Tempest.Scripting"
unrealscript := "Tempest.UnrealScript"
launcher := "Tempest.Launcher"
studio := "Tempest.Studio"
# `dotnet run` of the CLI; relative paths resolve from where just was invoked
tempest := 'dotnet run --project "' + justfile_directory() / "Tempest.CLI" + '" -p:RunWorkingDirectory="' + invocation_directory() + '"'
# R2 folder behind https://cdn.lowrezstudio.com/keys/ (rclone remote:bucket/path)
r2_keys := env("TEMPEST_R2_KEYS", "lowrezstudio:lowrezstudio-cdn/keys")

default:
    @just --list --unsorted

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

# Round-trip every package of a cooked game dir through the asset layer (tempest-assets) and report coverage
[group("unrealscript")]
upk-verify cooked *args: unrealscript-build
    {{unrealscript}}/target/release/upk verify "{{cooked}}" --classes {{args}}

# Run the package/asset CLI: just upk ls <pkg>, just upk dump <pkg> <idx|path> ...
[group("unrealscript")]
upk *args: unrealscript-build
    {{unrealscript}}/target/release/upk {{args}}

# ---------------------------------------------------------------------------
# .NET: Tempest.CLI, MarshalLib, MarshalLib.Database
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

# ---------------------------------------------------------------------------
# JS workspace: root pnpm workspace (Tempest.Launcher, Tempest.Studio)
# ---------------------------------------------------------------------------

# pnpm install for every JS package in the repo (enables corepack first)
[group("js")]
js-install:
    corepack enable
    pnpm install

# oxfmt over both Electron apps (root .oxfmtrc.json)
[group("js")]
js-fmt:
    pnpm fmt

# oxlint, type-aware, over both Electron apps (root .oxlintrc.json)
[group("js")]
js-lint:
    pnpm lint

# ---------------------------------------------------------------------------
# Tempest.Launcher (Tauri / Electron + SvelteKit)
# ---------------------------------------------------------------------------

# Electron dev app (needs the CLI built first)
[group("launcher")]
launcher-dev: build
    cd {{launcher}} && pnpm electron:dev

# Production Electron build -> Tempest.Launcher/dist/
[group("launcher")]
launcher-build:
    cd {{launcher}} && pnpm electron:build

# svelte-check (renderer) and electron typecheck (host); lint: just js-lint
[group("launcher")]
launcher-check: js-lint
    cd {{launcher}} && pnpm check
    cd {{launcher}} && pnpm check:electron

# Version bump + annotated tag (prompts; see AGENTS.md "Releasing")
[group("launcher")]
bump:
    cd {{launcher}} && pnpm bump

# ---------------------------------------------------------------------------
# Everything
# ---------------------------------------------------------------------------

# ---------------------------------------------------------------------------
# Tempest.Studio (Electron): package/asset editor over `upk serve`
# ---------------------------------------------------------------------------

# Electron dev app; builds the `upk` backend (cargo release) first
[group("studio")]
studio-dev:
    cd {{studio}} && pnpm electron:dev

# Production Electron build (bundles `upk` as a sidecar) -> Tempest.Studio/dist/
[group("studio")]
studio-build:
    cd {{studio}} && pnpm electron:build

# Ruffle v0.7.1 + Tempest.Studio/ruffle/*.patch (9-slice scaling) -> Tempest.Studio/vendor/ruffle (used instead of npm's when present)
[group("studio")]
studio-ruffle:
    {{studio}}/scripts/build-ruffle.sh

# svelte-check (renderer) and electron typecheck (host); lint: just js-lint
[group("studio")]
studio-check: js-lint
    cd {{studio}} && pnpm check
    cd {{studio}} && pnpm check:electron

# ---------------------------------------------------------------------------
# Tempest.StudioNative (GPUI): the Studio as a native app, tempest-assets in-process
# ---------------------------------------------------------------------------

# Run the native Studio (debug; optional game folder, else the last one)
[group("studio-native")]
studio-native *args:
    Tempest.StudioNative/scripts/fetch-ruffle.sh
    cd Tempest.StudioNative && cargo run -- {{args}}

# Release build -> Tempest.StudioNative/target/release/tempest-studio
[group("studio-native")]
studio-native-build:
    Tempest.StudioNative/scripts/fetch-ruffle.sh
    cd Tempest.StudioNative && cargo build --release

# Release bundle for this platform (app + tempest-cli + upk; .app, zip and DMG on macOS) -> Tempest.StudioNative/dist
[group("studio-native")]
studio-native-bundle out="":
    Tempest.StudioNative/scripts/bundle.sh {{out}}

# All verification that runs without a game: .NET build, scripting tests + clippy, unrealscript tests + clippy, JS checks
[group("all")]
check: build scripting-test scripting-check unrealscript-check launcher-check studio-check
