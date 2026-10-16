# AGENTS.md — Tempest

Compact context for OpenCode sessions. Prefer executable sources over this file; verify commands against the current repo.

## Repository layout

Mixed stack, split by directory:

- Root `.NET 10` solution: `Tempest.slnx`
  - `MarshalLib`, `MarshalLib.Database`
  - `Tempest.Protocol` — gRPC/protobuf contract, C# generated automatically by `Grpc.Tools`
  - `Tempest.CLI` — console app / launcher backend, references `MarshalLib`, `MarshalLib.Database`, `Tempest.Protocol`
  - `Tempest.Services` — ASP.NET Core gRPC server
- `Tempest.Utils/` — Zig 0.15.2 native tools (`asmloader`, `upkpatcher`, `mctsparser`)
- `Tempest.Scripting/` — Rust LuaJIT modding API DLL for Paladins 8.1 Win64 (`Tempest.Scripting64.dll`, built from macOS with MinGW-w64 via `scripts/package.sh`; shipped mods live in `mods/<name>/`), standalone Cargo workspace; see its `README.md`
- `Tempest.Launcher/` — Tauri v2 (Windows) / Electron (macOS + Linux) + SvelteKit 5 + Vite + TypeScript desktop frontend
- `Tempest.UnrealScript/` — Rust UnrealScript compiler/decompiler for the game's UPK packages (CLI `uscript`), standalone Cargo workspace; see its `AGENTS.md` before touching it

## Required tooling

- .NET SDK `10.0.100` (see `global.json`, `rollForward: latestFeature`)
- Zig `0.15.2`
- Rust stable (launcher, `Tempest.Scripting` Lua modding DLL, `Tempest.UnrealScript`)
- Node.js `>= 22` with Corepack enabled (only for the launcher)

## .NET commands

```bash
# Build the whole solution
dotnet build Tempest.slnx

# Run the CLI (from repo root)
dotnet run --project Tempest.CLI -- --help

# Run a CLI subcommand
dotnet run --project Tempest.CLI -- server --help

# Publish the CLI (AOT by default on Tempest.CLI.csproj)
dotnet publish -c Release Tempest.CLI
# Output: Tempest.CLI/bin/Release/net10.0/publish/

# Run the gRPC services server
dotnet run --project Tempest.Services
```

Notes:
- Building `Tempest.CLI` triggers an MSBuild target (`BuildZigInject`) that compiles `Tempest.CLI/inject/` with `zig build -Doptimize=ReleaseSmall`. Zig must be on `PATH`.
- `Tempest.Services` uses the default ASP.NET Core URL (`http://localhost:5000`) unless overridden by `Properties/launchSettings.json` or `ASPNETCORE_URLS`/`HTTP_PORTS`. The Dockerfile exposes ports 80/443; `docker-compose.yml` sets `ASPNETCORE_URLS=http://+:8080`.

## Task runner

The root `justfile` wraps every project's commands, grouped `utils` / `scripting` / `dotnet` / `launcher`; `just` lists them. `just check` runs all verification that needs no game (.NET build, scripting tests + clippy, unrealscript tests + clippy, launcher check + lint). The raw commands below are what the recipes run.

## Zig / native utils commands

```bash
just utils                 # Debug
just utils ReleaseFast     # any -Doptimize value
just utils-release         # ReleaseSmall (what the launcher bundle ships)
# or explicitly
zig build --build-file Tempest.Utils/build.zig -Doptimize=Debug
```

Artifacts land in `Tempest.Utils/zig-out/bin/`. The launcher bundle consumes the `asmloader-windows_*.dll` outputs. On macOS 26 with zig <0.16 the `zign` wrapper is needed; the justfile picks it up automatically when it is on `PATH` (`ZIG=... just utils` overrides).

## Tempest.Scripting commands

```bash
just scripting-test              # cargo test (host, no game image)
just scripting-check             # clippy, host + x86_64-pc-windows-gnu
just scripting-build             # Tempest.Scripting/scripts/package.sh -> dist/{scripting,console,dev,lib}.tempest
just scripting-run <game> [args] # build, then `tempest launch <game> --dll dist/scripting/dlls/Tempest.Scripting64.dll`
just scripting-install <game>    # build, then install all four packages
just scripting-dev <game>        # repack + reinstall only mods/dev/ (the scratch mod), no Rust build
```

The Win64 DLL cross-compiles with MinGW-w64 (`brew install mingw-w64`); `cross`/Docker is only the fallback. Logs go to `%TEMP%\tempest-scripting.log` (inside the Wine prefix when the game runs under Wine); the console window doubles as a Lua REPL. See `Tempest.Scripting/README.md`.

## Tempest.UnrealScript commands

```bash
just unrealscript-build          # cargo build --release -> Tempest.UnrealScript/target/release/uscript
just unrealscript-check          # cargo test + clippy (part of `just check`)
just unrealscript-verify <dir>   # decompile + recompile every script package of a cooked game dir, diff against originals
just uscript <args>              # run the CLI, e.g. `just uscript project init <cooked-dir> <project-dir>`
```

Correctness is defined as byte-identical output versus the game's own packages; `scripts/verify.sh` is the test suite. `Tempest.UnrealScript/AGENTS.md` documents the layout, the CLI, the verification loop and the game-specific compiler rules that must be preserved.

## Launcher commands

All run from `Tempest.Launcher/` (or via `just launcher-*` from the root):

```bash
corepack enable        # one-time
pnpm install

# Dev desktop app (Tauri + Vite)
# Build Tempest.CLI first; dev commands run it directly via `dotnet exec`
dotnet build Tempest.slnx
# Build the Zig utils first (`zig build` inside Tempest.Utils/), then copy the asmloader DLLs into src-tauri/binaries/ (gitignored).
# Without them `pnpm tauri dev` fails: resource path `binaries\asmloader-windows_x86.dll` doesn't exist
mkdir -p src-tauri/binaries
cp ../Tempest.Utils/zig-out/bin/asmloader-windows_x86*.dll src-tauri/binaries/
pnpm tauri dev
# Vite dev server is fixed to http://localhost:1420

# Production-like Tauri build
pnpm tauri build
# Bundles appear in src-tauri/target/release/bundle/

# Type-check / Svelte-check
pnpm check

# Format (oxfmt)
pnpm fmt

# Lint (oxlint)
pnpm lint

# Regenerate TS protobuf clients from ../Tempest.Protocol
pnpm proto-generate

# Electron dev/build (macOS/Linux)
pnpm electron:dev
pnpm electron:build
```

Notes:
- Package manager is `pnpm` only; `packageManager` is pinned in `package.json`.
- `pnpm install` runs `prepare`, which installs Husky hooks from the repo root.
- `pnpm tauri build` bundles external `tempest-cli` and `asmloader` binaries. If they are missing, the bundle step fails; the dev server does not need them. See `src-tauri/tauri.conf.json` and `.github/workflows/release.yml`.
- When installing or updating a Tauri plugin, the Electron layer must be kept in sync: add or update the corresponding shim in `Tempest.Launcher/src/lib/electron/bridge.ts` and implement the IPC handlers in `Tempest.Launcher/electron/`. Every plugin API must be mirrored 1:1 so the frontend compiles against the same interface on both runtimes.

## Code generation

Do not hand-edit generated files:

- `Tempest.Launcher/src/lib/paraglide/` — generated by `@inlang/paraglide-js` from `messages/*.json` via the Vite plugin. Source language is `en`; supported: `en`, `fr`, `es`, `pl`, `ru`.
- `Tempest.Launcher/src/lib/rpc/` — generated from `Tempest.Protocol/**/*.proto` by `pnpm proto-generate` (uses `protoc --ts_out` + `@protobuf-ts/plugin`).
- `Tempest.Protocol` C# outputs — generated automatically by `Grpc.Tools` on `dotnet build`.
- `Tempest.CLI/inject/zig-out/bin/inject{32,64}.exe` — generated by the `BuildZigInject` MSBuild target.

If you change `.proto` files, rebuild the solution and run `pnpm proto-generate`.

## Style / verification conventions

- Do not add or update translations (`messages/*.json` files for `fr`, `es`, `pl`, `ru`) unless explicitly told to. English source strings are fine to touch.
- `Tempest.Launcher/.editorconfig`: tabs for most files, spaces for `.rs`.
- Launcher lint: `oxlint` with config in `.oxlintrc.json`. Generated `src/lib/rpc/**` and `.svelte-kit/**` are ignored.
- Launcher is an SPA: `+layout.ts` disables SSR/prerender and enables CSR. Static adapter with `fallback: "index.html"`.
- TypeScript is strict; `tsconfig.json` extends `.svelte-kit/tsconfig.json`.
- No test projects currently exist; verification is build + `pnpm check` + `pnpm lint`.

## Release / CI

`.github/workflows/release.yml` builds on tags (`v*`) or manual dispatch:

- Windows → Tauri MSI
- macOS → Electron DMG
- Linux → Electron AppImage
- Installs .NET 10, Zig 0.15.2, Rust, Node 22, pnpm
- Uses `pnpm install --frozen-lockfile` and `tauri-apps/tauri-action@v0` (Windows) or `electron-builder` (macOS/Linux)
- Uploads artifacts and creates a draft GitHub release
- Release notes are pulled from the annotated tag's message (set by `pnpm bump`)

## Releasing

Use the bump script from `Tempest.Launcher/`:

```bash
pnpm bump
```

It will:
1. Prompt for the new version (suggesting the next pre-release number)
2. Prompt for multi-line patch notes
3. Update `package.json`, `tauri.conf.json`, `Cargo.toml`, `Cargo.lock`
4. Commit with the patch notes in the body
5. Create an annotated tag with the patch notes
6. Prompt to push to origin

The CI workflow then extracts the tag annotation and sets it as the GitHub Release body.

For local release builds, match the CI environment and ensure the sidecar binaries exist.
