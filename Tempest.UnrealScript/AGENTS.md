# Tempest.UnrealScript

Standalone Rust workspace: `tempest-unrealscript` compiler/decompiler (`uscript`),
`tempest-upk` container layer and `tempest-assets` asset/editor backend (`upk`).
Apply the [root guide](../AGENTS.md).

**Correctness means byte-identical output to the game's compiler, cooker and
SavePackage, not merely plausible output.** Retain raw/unknown bytes until an exact
serializer exists. Do not guess game-specific behavior or add heuristics that
only hide the current diff.

## Changes and verification

- For compiler changes, reproduce the dump diff, consult the engine source and
  confirm game deviations in the binary. Port the engine logic in its matching
  module; decompiled source must recompile to identical bytes.
- Read the [compiler rules](docs/compiler.md) for compiler/decompiler work and
  the relevant asset/engine topic below for serializers or editor work.
- Preserve sequential name interning/import creation inside one compile;
  `-j 1` is the byte-identity reference for parallel project builds.
- For backend API changes, update the Studio's
  [UpkMethods contract](../Tempest.Studio/electron/src/ipc/contract.ts) and callers.
- Keep mutations undoable and saved-copy/project/delta semantics intact.

From this workspace:

```sh
cargo build --release
cargo test --release
cargo clippy --release --all-targets
```

Start with affected crates/tests. Before finishing compiler changes, run the
round-trip harness on 0.57 and on 8.1 when available: every cooked package must
have **0 diff lines**. Asset changes need the affected round-trip verification:
0 mismatches, unparsed headers or raw values, and identical package images.
LZO recompression bytes are informational; image/chunk correctness still matters.
Read [verification](docs/verification.md) and [asset verification](docs/assets.md).
If game fixtures are unavailable, report that limitation rather than claiming parity.

## Read only when relevant

| Task | Reference |
| --- | --- |
| Crate/module layout, CLI entry points | [Architecture](docs/architecture.md) |
| Compilation, decompilation, flags, engine rules, gotchas | [Compiler](docs/compiler.md) |
| Properties, textures, audio, particles and package verification | [Assets](docs/assets.md) |
| Meshes, physics and animation sequences | [Meshes/animation](docs/meshes-and-animation.md) |
| Object/property edits, copying and creating | [Editing](docs/editing.md) |
| Delta format, bases, compression and shared objects | [Deltas](docs/deltas.md) |
| Maps, actors, Kismet, brushes and collision | [Maps/collision](docs/maps-and-collision.md) |
| Matinee evaluation and editing | [Matinee](docs/matinee.md) |
| Light baking and engine encoding | [Lighting](docs/lighting.md) |
| Havok tagfiles and nav mesh edits | [Nav mesh](docs/navmesh.md) |
| `serve`, framing, workspace/cooker and performance | [Backend](docs/backend.md) |
| Movie hot reload/baselines | [Hot reload](docs/hot-reload.md) |
| MCTS formats, assembly, skins and content creation | [Assembly](docs/assembly.md) |
| SWF/GFx serialization, rendering and ActionScript | [Movies](docs/movies.md) |

Cross-project mod policy: [signing/trust](../Tempest.CLI/docs/mods.md).
