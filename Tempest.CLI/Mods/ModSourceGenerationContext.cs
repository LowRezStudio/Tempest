using System.Collections.Generic;
using System.Text.Json.Serialization;

namespace Tempest.CLI.Mods;

[JsonSerializable(typeof(ModRecord))]
[JsonSerializable(typeof(ModInstallResult))]
[JsonSerializable(typeof(ModListResult))]
[JsonSerializable(typeof(ModBulkResult))]
[JsonSerializable(typeof(List<ModRecord>))]
internal partial class ModSourceGenerationContext : JsonSerializerContext
{
}
