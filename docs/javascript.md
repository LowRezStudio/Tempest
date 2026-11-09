# Shared Electron workspace

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

- Package manager is `pnpm` only; `packageManager` is pinned in the root `package.json`.

- One pnpm workspace at the root (`pnpm-workspace.yaml`: `Tempest.Launcher`,
  `Tempest.Launcher/electron`, `Tempest.Studio`, `Tempest.Studio/electron`), one `pnpm-lock.yaml`.
  Scripts can still be run inside a package directory (`cd Tempest.Launcher && pnpm electron:dev`);
  pnpm finds the workspace root.

- `pnpm install` runs the root `prepare`, which installs the repo-root Husky hook
  (`.husky/pre-commit`: formats the staged JS/TS/JSON/CSS/Svelte files with the root oxfmt).

- Shared tool versions live in the `catalog:` of the root `pnpm-workspace.yaml` (`husky`, `oxfmt`,
  `oxlint`, `oxlint-tsgolint`, `vite`); each package that runs one declares it as `"catalog:"`,
  which puts its bin on that package's `PATH`: `vite` in both Electron hosts (their dev/build
  scripts run the renderer), `oxfmt` + `oxlint` in both apps, and `oxfmt` in the launcher too
  (`proto-generate` formats its output with it). `workspace:*` links workspace packages only, so it
  does not apply to external tools.

- `pnpm fmt` / `pnpm lint` at the root run `pnpm -r run fmt|lint`, i.e. each app formats and lints
  its own tree (its config is found in the parent directory). An app's `lint` runs `svelte-kit sync`
  first, because type-aware linting reads its generated `.svelte-kit/tsconfig.json` — so `pnpm -C
  Tempest.Studio lint` works on a fresh clone too.

- The Electron host is TypeScript in `electron/src/`, compiled by `tsc` into `electron/dist/` (the
  dev and build scripts do this automatically; `pnpm check:electron` / `pnpm -C electron typecheck`
  verifies it). Restart `electron:dev` to pick up host changes. The IPC surface is typed end-to-end
  by `electron/src/ipc/contract.ts`, which the renderer imports as types — adding a channel means
  adding it to the contract, then the handler and the renderer call site all get checked.

- The tray, close-guard (hide-to-tray), dialogs, filesystem operations (platform detection, instance
  paths, mod downloads/previews, settings snapshots) all live in the Electron host; the renderer
  only orchestrates.

- `pnpm electron:build` bundles external `tempest-cli`, `upk` and `asmloader` binaries built by
  `scripts/build-cli.js` into `electron/resources/binaries`; the dev server does not need them (the
  cooker uses the Rust release build of `upk` from `Tempest.UnrealScript/target/release`,
  overridable with `TEMPEST_UPK`).
