# Launcher architecture and features

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

- `Tempest.Launcher/` — Electron + SvelteKit 5 + Vite + TypeScript desktop frontend (Windows, macOS
  and Linux); the Electron host (`electron/src/`) is TypeScript compiled by `tsc` to
  `electron/dist/` (gitignored), with a single typed IPC contract (`electron/src/ipc/contract.ts`)
  shared by host and renderer — all filesystem/process/tray work happens on the host

- The renderer talks to the Electron main process through `src/lib/platform/api.ts` (over
  `window.electronAPI`); IPC handlers live in `Tempest.Launcher/electron/src/`. Bundled default mods
  live in `electron/resources/mods/`.

- `tempest://install?mod=<id>&version=<v>` deep links (from mods.lowrezstudio.com) are handled by
  the Electron host (`setAsDefaultProtocolClient("tempest")` + `open-url`/`second-instance` in
  `electron/src/`, registered in `ipc/contract.ts`) and `src/lib/mods/deeplink.svelte.ts`; the
  `protocols:` entry in `electron/electron-builder.yml` registers the scheme for installed builds.
  The mod repository client is `src/lib/queries/repo.ts` (URL from the `modRepositoryURL` setting).

- **Crash reports** (`electron/src/crash.ts`, `crash-logs.ts`, `src/lib/core/crash.svelte.ts`,
  `CrashReportDialog.svelte`): `tempest launch` prints `[game-logs] <log folder>\t<temp folder>`
  when the game starts (`Launcher/GameLogs.cs`, resolves the Wine prefix) and exits with the game's
  exit code. When a launch ends without the player stopping it, `crash:inspect` reads the session's
  `Launch*.log`: the engine's error path in the log (`StaticShutdownAfterError`, `=== Critical
  error: ===`, `appRequestExit(1)`), or a non-zero exit code without `Exit: Exiting.`, is a crash
  (an access violation never reaches the engine's handler, the log just stops). The dialog offers
  **Report logs** and **Save a copy**; the redacted logs are staged host-side at inspection, because
  the next launch rotates `Launch.log`. Nothing is sent without the button; the minidump (not
  redactable) is opt-in. The report is a Discord webhook call (`payload_json` + `files[0]`, a zip)
  to the relay, a Cloudflare Worker in the separate `tempest-crash-reports` repository that holds
  the webhook (`REPORT_URL` in `crash.ts`; `TEMPEST_CRASH_REPORT_URL` overrides it). What must not
  leave the machine is `redactLog` in `crash-logs.ts`: Vivox lines hold sign-in tokens, `Init:`
  lines the OS user and computer names.

- **Mod updates** (`src/lib/mods/updates.svelte.ts`, `ModUpdatesPrompt.svelte`): at start and each
  time the window regains focus (`+layout.svelte`; repository answers are reused for 60 s) every
  prepared instance is checked. With **Automatically install updates** (`autoInstallModUpdates`, on
  by default; the update popup and Settings, mods only: the launcher itself is only prompted, also
  on every focus) enabled mods install in the background, each version tried once per session; what
  is left is shown in the popup. A version the user downgraded to is left alone (`holdModVersion`).
  **Hot reload**: `mod install --defer` reports `HotReloadable` when the package and what it
  replaces are only Lua scripts, its dependencies are in place and `tempest-scripting` >= 1.0.6 is
  installed (`CanHotReload` in `ModV2Installer.cs`). While the game runs a manual install of such a
  mod goes straight in and the runtime reloads it (`watch.rs` follows `mods.json`); automatic
  updates (`waitForGame`) only download and wait in the install queue, with an "Install and reload"
  button in the popup and the queue panel, else they install when the game closes.
