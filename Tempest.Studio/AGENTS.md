# Tempest.Studio

Electron + SvelteKit 5 package/asset editor; separate from the launcher and the
native GPUI editor. Apply the [root guide](../AGENTS.md) and
[shared JS conventions](../docs/javascript.md). Keep the Studio's purple icon distinct.

## Architecture and edit safety

- OS work stays in the [Electron host](electron/src/); package work runs in the
  Rust `upk serve` backend, one server/workspace per window.
- Keep [IPC/UpkMethods](electron/src/ipc/contract.ts), backend methods and
  [renderer RPC](src/lib/studio/rpc.ts) in sync. Read the
  [backend guide](../Tempest.UnrealScript/AGENTS.md) before changing Rust.
- Preserve framed binary replies, columnar tables and `$state.raw` large datasets.
- [Studio state](src/lib/studio/state.svelte.ts) owns selection and dirty packages.
  Edits are in memory and undoable; Save writes in place, Export a copy leaves
  edits unsaved, workspace/project files keep edits for later.
- In-place writes need the existing confirmation/base-recording path. Closing a
  project must unload its edits so they cannot accidentally enter the game folder.
- Use the [command registry](src/lib/studio/commands.svelte.ts) and
  [action definitions](src/lib/studio/actions.ts) for keyboard, palette and menu
  actions. Dialogs and confirmations go through existing UI state helpers.
- Follow existing Svelte 5, Tailwind + daisyUI patterns; preserve pane resizing,
  compact overlays, focus, history and tab behavior. Design details belong in
  the [UI reference](docs/ui.md), not this file.

## Commands (repository root)

```sh
pnpm -C Tempest.Studio electron:dev
pnpm -C Tempest.Studio check
pnpm -C Tempest.Studio check:electron
pnpm -C Tempest.Studio lint
```

Dev builds the Rust release backend and host, then starts Vite on 1421 + Electron.
Production: `pnpm -C Tempest.Studio electron:build`.
The backend can be overridden with `TEMPEST_UPK`; restart Electron for host changes.

## Read only when relevant

| Task | Reference |
| --- | --- |
| Backend lifecycle, startup, IPC and tables | [Architecture/development](docs/architecture.md) |
| Shell, keyboard, menus, tabs and panes | [UI/interaction guidelines](docs/ui.md) |
| Packages, properties, textures, meshes, audio, particles, animations, scripts | [Asset editing](docs/assets.md) |
| SWF/GFx, Ruffle, stage, timeline and ActionScript | [Movies](docs/movies.md) |
| Assembly tables, skins and content creation | [Assembly](docs/assembly.md) |
| Maps, Kismet, Matinee, collision, lighting and nav mesh | [Maps](docs/maps.md) |
| Projects, `.twrk`, mod cooker, saving and undo | [Mods/workspaces](docs/mods-and-workspaces.md) |
| Screenshots, smoke harness or MCP | [Testing](docs/testing.md) |
| Updater, settings or release (`studio-v*`) | [Releases/updates](docs/releases.md) |

Never save into the user's game folder or publish from a test run. Use isolated
settings and MCP ports for test instances.
