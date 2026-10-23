# Tempest Studio

Desktop editor for Paladins packages (`.upk`): browse a cooked folder, inspect and edit object properties, preview and replace textures, meshes, sounds and UI movies, copy objects between packages, verify round trips and prepare mod folders.

The UI is Electron + SvelteKit; everything that touches packages runs in `upk serve`, the Rust backend from `../Tempest.UnrealScript` (`crates/tempest-assets`).

```bash
pnpm install
pnpm electron:dev      # builds the Rust backend in release mode first
pnpm electron:build    # installers in dist/
```

See the root `AGENTS.md` for conventions and `Tempest.UnrealScript/AGENTS.md` for the backend protocol.
