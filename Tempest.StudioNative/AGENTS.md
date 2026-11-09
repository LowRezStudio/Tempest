# Tempest.StudioNative

Native Rust + GPUI Studio, ported alongside the shipping Electron Studio.
Apply the [root guide](../AGENTS.md). This is its own Cargo package/toolchain,
not part of the UnrealScript or Scripting workspaces.

## Core rules

- Use the pinned [rust-toolchain.toml](rust-toolchain.toml); GPUI needs its newer
  Rust version. Dependencies are pinned in [Cargo.toml](Cargo.toml).
- [backend.rs](src/backend.rs) runs the asset `Server` in-process on a worker
  thread; keep binary payloads as bytes and account for in-flight work with
  `WORK`/`Busy`. Backend Rust changes also require the
  [UnrealScript guide](../Tempest.UnrealScript/AGENTS.md).
- Do not update an entity while it is already being updated. Driver code updates
  Assembly/Mod entities directly, not recursively through `Studio::drive`.
- `InputState::set_value` is not immediately visible to `value()`; code setting
  a field must also apply the value itself.
- Every user action belongs in [commands.rs](src/studio/commands.rs) (`Cmd`),
  shared by keys, palette, menus and shortcut help. Use the palette for object picks.
- Follow existing GPUI/gpui-component patterns, registered Lucide icons and
  Catppuccin themes. Design, layout and interaction details live in [UI](docs/ui.md).
- Preserve in-memory edits, undo, save confirmations, project autosave/unloading
  and recovery. Report user-visible errors through the existing toast path.
- Retain Electron/backend behavior when porting a view; do not silently change
  file formats or editor semantics.

## Commands

From the repository root:

```sh
just studio-native
just studio-native-build
just studio-native-bundle
```

These fetch the patched Ruffle sources before building/running.
Run `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` from this
project for Rust changes, after fetching Ruffle with the existing script if absent.
Use the relevant driver scenario for behavior changes.

## Read only when relevant

| Task | Reference |
| --- | --- |
| GPUI dependencies and in-process backend | [Architecture](docs/architecture.md) |
| Shell, settings, theme, panes and windows | [UI/design guidelines](docs/ui.md) |
| Packages, previews, creation wizard, scripts and particles | [Assets](docs/assets.md) |
| Assembly, content creation, skins and scaffolding | [Assembly](docs/assembly.md) |
| Mod cooker, signing/publishing, projects and workspaces | [Mods/workspaces](docs/mods-and-workspaces.md) |
| SWF editor, Ruffle, stage, timeline and ActionScript | [Movies](docs/movies.md) |
| Maps, Kismet, Matinee, collision, lighting and nav mesh | [Maps](docs/maps.md) |
| MCP, driver, probes and screenshots | [Testing](docs/testing.md) |
| Bundle, associations, self-update and release (`studio-native-v*`) | [Releases](docs/releases.md) |

Tests must use their own `HOME`/settings, MCP port and output directory. Bring the
window forward before screenshots. Never save into the game folder or publish.
