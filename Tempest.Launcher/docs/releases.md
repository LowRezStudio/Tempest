# Launcher releases and CI

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Release / CI

`.github/workflows/release.yml` builds on tags (`v*`) or manual dispatch:

- Windows → Electron NSIS installer
- macOS → Electron DMG
- Linux → Electron AppImage + deb + rpm
- Installs .NET 10, Zig 0.15.2, Node 22, pnpm
- Uses `pnpm install --frozen-lockfile` and `electron-builder` (`--publish always` on tags,
  `--publish never` on dispatch)
- Each app's workflow installs only its own dependency graph, not the other app's: `pnpm --filter
  tempest.launcher --filter tempest-launcher-electron install --frozen-lockfile` (the Studio's is
  the mirror image, `sidecar-cache.yml` filters to `tempest.launcher` because `scripts/build-cli.js`
  only imports `dax`). The root project is installed either way, so `prepare` (husky) runs.
- Uploads artifacts and creates a draft GitHub release
- Release notes are pulled from the annotated tag's message (set by `pnpm bump`)
- The bundled CLI sidecar (`scripts/build-cli.js`: AOT `tempest-cli`, `upk`, the asmloader and faststart DLLs) is
  built by three independent jobs in parallel, with the zig build cache restored from
  `sidecar-cache.yml` (a tag run can only see caches saved on master or by itself, so that workflow
  warms them on master pushes that touch `Tempest.Utils/**`, weekly, and by hand).

## Releasing

Use the bump script from `Tempest.Launcher/`:

```bash
pnpm bump
```

It will:
1. Prompt for the new version (suggesting the next pre-release number)
2. Prompt for multi-line patch notes
3. Update `package.json` and `electron/package.json`
4. Commit with the patch notes in the body
5. Create an annotated tag with the patch notes
6. Prompt to push to origin

The CI workflow then extracts the tag annotation and sets it as the GitHub Release body.

For local release builds, match the CI environment and ensure the sidecar binaries exist.
