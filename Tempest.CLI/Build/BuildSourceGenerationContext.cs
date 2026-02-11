using System.Text.Json.Serialization;

namespace Tempest.CLI.Build;

[JsonSerializable(typeof(BuildInfo))]
internal partial class BuildSourceGenerationContext : JsonSerializerContext
{
}
