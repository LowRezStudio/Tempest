# Tempest.CLI

.NET 10 console app/launcher backend, referencing the marshaling and database
libraries. Apply the [root guide](../AGENTS.md).

## Build and checks (repository root)

```sh
dotnet build Tempest.slnx
dotnet run --project Tempest.CLI -- --help
dotnet run --project Tempest.CLI -- mod --help
dotnet publish -c Release Tempest.CLI
```

The [project](Tempest.CLI.csproj) enables AOT publishing. Building the CLI also
builds the Zig injection executables; Zig 0.16.0 must be on `PATH`.
Do not hand-edit generated injection output. See
[shared development commands](../docs/development.md).

## Mod and launch invariants

- Read [mod deltas, signing and trust](docs/mods.md) before changing installation,
  removal, enable/disable, launch, packaging, dependencies or key handling.
- Rebuild derived game files as pristine base + enabled deltas in install order;
  preserve backup restoration, drift reporting and progress output.
- Code-bearing packages need valid signatures and the existing trust policy;
  revocations win. Keep path containment and game-folder casing protections.
- DLL injection must match the game's PE architecture and respect installed
  mod signature/revocation state.
- Preserve launched-process package hashes, game log reporting and exit codes;
  the launcher consumes these outputs.
- Keep the TypeScript callers/IPC and `upk` sidecar behavior compatible when
  changing CLI output or mod commands.

For release/sidecar bundling, use the
[Launcher release reference](../Tempest.Launcher/docs/releases.md).
