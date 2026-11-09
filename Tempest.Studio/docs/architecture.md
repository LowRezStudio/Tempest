# Studio development and backend

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

- `Tempest.Studio/` — Electron + SvelteKit 5 desktop **package/asset editor** (Windows, macOS,
  Linux; its own purple "STUDIO" icon in `electron/resources/icons/` and `static/`, distinct from
  the launcher's), a separate app from the launcher with the same stack and conventions (TypeScript
  host in `electron/src/` compiled by `tsc`, typed IPC contract in `electron/src/ipc/contract.ts`,
  pnpm, oxlint/oxfmt, Tailwind + daisyUI). Its backend is the Rust `upk serve` process from
  `Tempest.UnrealScript` (JSON lines over stdio; built by `cargo build --release` in dev, bundled as
  a sidecar in `electron/resources/binaries/` by the production build)

## Tempest.Studio commands

Run from the repository root (same root workspace as the launcher; `just studio-*` wraps these):

```bash
pnpm install                       # once, at the root
pnpm -C Tempest.Studio electron:dev     # builds `upk` (cargo release) + host (tsc), starts vite (port 1421) + Electron
pnpm -C Tempest.Studio electron:build   # release build: upk sidecar + renderer + host -> dist/
pnpm -C Tempest.Studio check            # svelte-check
pnpm -C Tempest.Studio check:electron   # host typecheck (or `pnpm check:studio` from the root for both)
```

Lint and format run per app (`pnpm -C Tempest.Studio lint` / `fmt`), or for the whole repo with
`pnpm lint` / `pnpm fmt` at the root (see [shared JS conventions](../../docs/javascript.md)).

- The host finds the backend at `$TEMPEST_UPK`, else `<resources>/binaries/upk` (packaged), else
  `../Tempest.UnrealScript/target/release/upk` (dev). `electron/src/upk.ts` spawns one `upk serve`
  per window (a server holds one workspace, so every window can work on its own game folder;
  `upkFor(webContents)`), numbers requests, resolves promises by id and forwards event lines
  (`search.progress`) as `upk:event` to the owning window. Responses are framed (`protocol {frames:
  true}` handshake): a header line with `len` and the payload lengths `bin`, the result JSON bytes,
  then the raw binary payloads (previews, meshes, animations, audio) where the JSON holds `{"$bin":
  n}` markers; the host forwards the bytes and the renderer parses the JSON once
  (`src/lib/studio/rpc.ts`). Package tables travel as columns (`ExportColumns`, expanded into
  `ExportRow`s by `src/lib/studio/tables.ts`; names via `package.names`, flag names via
  `package.export`) and the open package is `$state.raw`, so a 65k-export script package opens in
  under a second. **New window** (toolbar, Ctrl+Shift+N, the welcome screen's recent list) calls
  `window:new`, optionally with a `dir`; a new window pulls what it should open with
  `window:startup` once its renderer is up (kept by the host until applied, so a dev reload replays
  it). Files opened from the OS go to the focused window (`open-files` event once it is ready, else
  its startup).

- `electron/src/ipc/contract.ts` also holds the JSON shapes `upk serve` returns (`UpkMethods`); the
  renderer calls them through `src/lib/studio/rpc.ts`. Adding a method means: `serve.rs` +
  `UpkMethods` + the call site.

- State lives in `src/lib/studio/state.svelte.ts` (workspace, open package tables, selection, dirty
  packages); edits are applied in the backend's in-memory package and written on Save (in place,
  compressed/obscured like the original) or Save as.

- Opening a `.upk` from the OS (file association, argv) or with **Open file**: the game folder
  (class database, texture caches) is the file's own folder when it is one, else the last workspace
  used; otherwise the file waits on the welcome screen until a folder is picked. Packages outside
  the workspace folder are listed as external ("ext") entries in the package pane
  (`studio.extraPackages`) and edited like any other.
