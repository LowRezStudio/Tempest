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
  the webhook (`REPORT_URL` in `crash.ts`; `TEMPEST_CRASH_REPORT_URL` overrides it). `redactLog`
  in `crash-logs.ts` removes recognizable credentials, account IDs, emails and IP addresses, but
  keeps account, player and computer names and file paths.
  Submissions carry a random `installation_id`; the relay groups them in one private Discord
  forum thread per ID (older clients share a legacy thread). Settings > About displays/copies it.
  Windows stores it in `HKCU\Software\LowRezStudio\TempestCrashReports\InstallationId`; macOS/Linux
  use `LowRezStudio/TempestCrashReports/installation-id` under `~/Library/Application Support`
  or `~/.local/share`. It survives launcher reinstalls and is per OS user, not a hardware ID.
  The launcher's `TEMPEST_CRASH_DIAGNOSTICS=1` adds a module snapshot path and dev-mod directory
  to `[game-logs]`; older CLIs still launch without diagnostics. Rebuild the CLI to enable them.
  A native .NET task samples Windows loader-visible modules every second before injection,
  retaining paths and on-disk SHA-256 only. It writes the snapshot when observation finishes;
  shutdown allows ten seconds before cancelling collection. Snapshots in temp `TempestCrashDiagnostics`
  are consumed at inspection; leftovers older than two days are pruned. No DLL binaries or
  certificate metadata are collected. Short-lived/manual-mapped modules may be missed; reports
  are client-supplied, not trusted evidence or an automatic denylist.
  Dev-mod logs/source/config come from `mods` or runtime `[dev] mods` (`TEMPEST_SCRIPT_CONFIG`
  supported). Collection skips symlinks, unrelated folders, binaries and obvious secret files,
  with limits of 2 MiB/file, 8 MiB total and 500 entries. Collection errors/omissions are shown
  and archived; recognizable credentials, IP addresses, machine IDs and serial numbers are
  redacted, but names and file paths may remain. Save a copy to review.
  Optional, default-on `system-information.txt` uses fast Node OS APIs for OS/architecture,
  CPU and RAM only, without DxDiag or device/driver scans. Both send/save use the same attachments.
  Focused tests: `node --test Tempest.Launcher/electron/tests/*.test.mjs` (Node 22.18+).

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
