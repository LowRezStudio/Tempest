# MarshalLib

.NET 10 marshaling library used by the CLI. Apply the [root guide](../AGENTS.md).

- Preserve existing serialization behavior and compatibility with database and
  CLI consumers.
- Follow the existing C# patterns and root formatting configuration.
- Validate library changes with `dotnet build MarshalLib` from the repository root.
  Build `Tempest.slnx` when shared APIs or wire behavior affect consumers.

See [shared .NET development](../docs/development.md),
[database guide](../MarshalLib.Database/AGENTS.md) and
[CLI guide](../Tempest.CLI/AGENTS.md) for cross-project changes.
