# Native Studio: releases

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Release, self-update and file associations

**Release and updates**: `scripts/bundle.sh [out]` (`just studio-native-bundle`) builds the app,
`upk` and the AOT `tempest-cli` (prebuilt ones via `STUDIO_BIN`/`UPK`/`CLI_DIR`) and lays them out
side by side (the cooker finds `tempest-cli` next to the exe, `tempest-cli` finds `upk`): on macOS
`Tempest Studio Native.app` (`com.lowrezstudio.tempest.studio-native`, `.upk`/`.twrk` document types
in its Info.plist, signed with `APPLE_SIGNING_IDENTITY` or ad-hoc, notarized with
`APPLE_NOTARY_PROFILE`) as a zip and a DMG, elsewhere a zip of a folder holding the
`.tempest-studio-bundle` marker; every zip gets a `<zip>.sha256`.
`.github/workflows/studio-native-release.yml` runs it on `studio-native-v*` tags (its own prefix:
the Electron Studio updates from `studio-v*`), attaching the files to a draft release; release with:
bump `Tempest.StudioNative/Cargo.toml`, `git tag -a studio-native-vX.Y.Z -m "<notes>"`, push,
publish the draft. **Self-update** (`src/update.rs`, UI `studio/updates.rs`): a bundled build (a
`.app`, or a folder with the marker; never `cargo run`) asks the GitHub API for the newest published
`studio-native-v*` release 5 s after start (Settings: turn off, Check now), shows a banner in every
window; Restart to update downloads `Tempest-Studio-Native-<v>-<os>-<arch>.zip` next to the install,
checks its `.sha256`, unpacks it (ditto on macOS), keeps every window's work (a project saves its
file, other unsaved edits go to the recovery workspace file, offered at the next start), renames the
running copy aside and moves the new one in (file by file on Windows/Linux), quits, and a shell
helper waits for the process, deletes the old copy and starts the new; leftovers are removed at
start. A copy in a folder the user cannot write offers the download page instead. **File
associations** on Windows (HKCU "Open with" ProgIDs `TempestStudioNative.upk|twrk`, the default app
is left alone) and Linux (`~/.local/share/applications/tempest-studio-native.desktop` + a MIME
package) are written by `src/shell.rs` the first time a bundle runs from a place. Release builds on
Windows have no console (`windows_subsystem`) and carry the icon (`build.rs`, winresource).
