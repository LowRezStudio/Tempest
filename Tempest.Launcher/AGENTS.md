# Tempest.Launcher

Electron + SvelteKit 5 + Vite + strict TypeScript desktop launcher. Apply the
[root guide](../AGENTS.md) and [shared JS conventions](../docs/javascript.md).

## Boundaries and conventions

- Filesystem, process, tray, dialogs and other OS work belong in the TypeScript
  [Electron host](electron/src/), not the renderer.
- Keep IPC typed end-to-end through [contract.ts](electron/src/ipc/contract.ts).
  Renderer calls use [platform/api.ts](src/lib/platform/api.ts).
- Host output in `electron/dist/` is generated; restart dev Electron after host changes.
- This is an SPA: SSR/prerender disabled, CSR enabled, static fallback `index.html`.
- Follow existing Svelte 5 patterns and Tailwind + daisyUI components.
- Edit English messages in [messages/en.json](messages/en.json), not generated
  [paraglide](src/lib/paraglide/) or translated catalogs without explicit approval.
- [RPC clients](src/lib/rpc/) are generated from the shared protobuf contract.
- Crash reporting must remain user-initiated; minidumps are opt-in. Preserve credential and
  identifier redaction in [logs](electron/src/crash-logs.ts), and warn that names and paths may remain.
- Preserve mod trust warnings, deferred installs/hot reload and held downgraded
  versions when modifying mod installation or updates.

## Commands (repository root)

```sh
dotnet build Tempest.slnx
pnpm -C Tempest.Launcher electron:dev
pnpm -C Tempest.Launcher check
pnpm -C Tempest.Launcher check:electron
pnpm -C Tempest.Launcher lint
pnpm -C Tempest.Launcher proto-generate
```

Dev uses port 1420 and runs the built CLI via `dotnet exec`.
Production: `pnpm -C Tempest.Launcher electron:build`.
Format with the app's `fmt` script; avoid unrelated formatting.

## Read only when relevant

| Task | Reference |
| --- | --- |
| Setup, dev/build and generation | [Development](docs/development.md) |
| IPC, deep links, crashes and mod updates | [Architecture/features](docs/architecture.md) |
| Mod packaging, deltas, signing or trust | [Shared mod rules](../Tempest.CLI/docs/mods.md) |
| Release, `pnpm bump`, sidecars or CI (`v*`) | [Releases](docs/releases.md) |
