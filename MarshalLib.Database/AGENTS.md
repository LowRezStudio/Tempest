# MarshalLib.Database

.NET 10 database library used by the CLI. Apply the [root guide](../AGENTS.md).

- Preserve existing database/marshaling behavior and CLI compatibility.
- Follow existing C# patterns and root formatting configuration.
- Validate library changes with `dotnet build MarshalLib.Database` from the
  repository root. Build `Tempest.slnx` when shared APIs affect consumers.

See [shared .NET development](../docs/development.md),
[marshaling guide](../MarshalLib/AGENTS.md) and
[CLI guide](../Tempest.CLI/AGENTS.md) for cross-project changes.
