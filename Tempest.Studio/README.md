# Tempest Studio

Desktop editor for Paladins packages (`.upk`): browse a cooked folder, inspect and edit object properties, preview and replace textures, meshes, sounds and UI movies, copy objects between packages, verify round trips and prepare mod folders.

The UI is Electron + SvelteKit; everything that touches packages runs in `upk serve`, the Rust backend from `../Tempest.UnrealScript` (`crates/tempest-assets`).

```bash
# from the repository root: one pnpm workspace for the launcher and the Studio
pnpm install
pnpm -C Tempest.Studio electron:dev      # builds the Rust backend in release mode first
pnpm -C Tempest.Studio electron:build    # installers in dist/
```

See the [Studio agent guide](AGENTS.md) for conventions and topic references,
the [root guide](../AGENTS.md) for shared rules, and the
[backend reference](../Tempest.UnrealScript/docs/backend.md) for the backend protocol.
