# Tempest agent guide

Start here, then read the `AGENTS.md` in each project you will change. For
cross-project work, read each affected project's guide. Detailed references are
on-demand: open only the document/section relevant to the task, not the whole tree.
Prefer executable sources over documentation; verify commands against manifests
and the [task runner](justfile).

## Project routing

| Area | Guide | Stack / responsibility |
| --- | --- | --- |
| Launcher | [Tempest.Launcher](Tempest.Launcher/AGENTS.md) | Electron + SvelteKit 5 desktop launcher |
| Studio | [Tempest.Studio](Tempest.Studio/AGENTS.md) | Electron + SvelteKit 5 package/asset editor |
| Native Studio | [Tempest.StudioNative](Tempest.StudioNative/AGENTS.md) | Rust + GPUI editor; separate from Electron Studio |
| Scripting | [Tempest.Scripting](Tempest.Scripting/AGENTS.md) | Rust + LuaJIT modding DLLs and shipped Lua mods |
| UnrealScript / assets | [Tempest.UnrealScript](Tempest.UnrealScript/AGENTS.md) | Rust compiler/decompiler, `upk`, Studio backend |
| CLI / mod installation | [Tempest.CLI](Tempest.CLI/AGENTS.md) | .NET launcher backend, server and mod tooling |
| Native injection utilities | [Tempest.Utils](Tempest.Utils/AGENTS.md) | Zig `asmloader` DLLs |
| Protocol | [Tempest.Protocol](Tempest.Protocol/AGENTS.md) | gRPC/protobuf contract |
| Marshaling | [MarshalLib](MarshalLib/AGENTS.md) | .NET marshaling library |
| Database | [MarshalLib.Database](MarshalLib.Database/AGENTS.md) | .NET database library |

## Shared conventions

- Follow the root [.editorconfig](.editorconfig): tabs for most files, spaces for Rust.
- Do not hand-edit generated outputs; edit their sources and regenerate. A `.proto`
  change requires `dotnet build Tempest.slnx` and
  `pnpm -C Tempest.Launcher proto-generate`.
- Do not add/update French, Spanish, Polish or Russian message catalogs unless
  explicitly requested. English source strings may change.
- Never commit private signing keys, credentials or tokens.
- Validate the affected project with its existing checks. Use focused checks first;
  `just check` is the broad no-game verification, not the default for every edit.
- Tests must not save into a user's game folder or publish mods. Isolate test app
  settings, outputs and MCP ports from the user's running apps.

## Shared references

- [Tooling, .NET commands and generation](docs/development.md):
  .NET 10, Zig 0.15.2, separate Rust workspaces, Node >= 22.
- [JavaScript workspace conventions](docs/javascript.md):
  pnpm only, one root workspace/lockfile, shared oxlint/oxfmt configuration.
- [Mod format, deltas, signing and trust](Tempest.CLI/docs/mods.md):
  read when changing mod producers, installation, dependencies or trust policy.
- Release procedures live in the relevant project's guide; launcher, Studio and
  Native Studio have distinct tag prefixes.

## Keeping these guides small

Keep root and project `AGENTS.md` files under 100 lines and roughly 6 KB each.
Put project-specific rules in the project's guide, and feature inventories,
engine rules, design/interaction details, test-driver commands and release
procedures in linked topic documents. Update the relevant document rather than
appending another feature history to an always-loaded guide.
