# Studio releases and updates

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

`.github/workflows/studio-release.yml` does the same for **Tempest Studio** on `studio-v*` tags (or
manual dispatch): draft release titled "Tempest Studio <version>", then Windows NSIS, macOS DMG and
Linux AppImage/deb/rpm builds (Rust stable builds the `upk` sidecar, `pnpm check:studio`/`pnpm lint`
and `cargo test -p tempest-assets` run first) attached with `gh release upload`. Release a Studio
version with: bump `Tempest.Studio/package.json` + `electron/package.json`, commit, `git tag -a
studio-vX.Y.Z -m "<notes>"`, push the tag, then publish the draft.

- **Updates and settings**: the Studio updates itself like the launcher (`electron/src/updater.ts`,
  electron-updater): Studio releases share the repository and are never the latest release, so
  `updater:check` asks the GitHub API for the newest published `studio-v*` release, then points
  electron-updater's generic provider at that release's download folder, where `studio-release.yml`
  attaches the update files (`latest.yml`, `latest-mac.yml`, `latest-linux.yml`, blockmaps;
  electron-builder writes them because `publish` is a generic provider with that URL, `--publish
  never`). Windows and Linux (AppImage, deb/rpm with pkexec) download (`updater:download`,
  `updater:progress` events) and install (`updater:install`, relaunch waits for the old process:
  `waitForUpdatedParent`); macOS and other installs get a link to the release page. The renderer
  (`src/lib/studio/updater.svelte.ts`) checks 5 s after start unless turned off, shows
  `UpdateBanner.svelte`, and before installing saves the project or offers a workspace file for
  unsaved edits. **Settings** (`SettingsDialog.svelte`, activity bar gear, Mod+,): updates (version,
  check, download/restart, release notes, check at start), theme and zoom, the in-place save warning
  (`ui.confirm`'s `remember` keys), signing and publishing (`ModAccount.svelte`), the MCP server,
  recent lists.
