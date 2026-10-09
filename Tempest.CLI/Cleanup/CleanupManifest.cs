using System.Text.Json.Serialization;

namespace Tempest.CLI.Cleanup;

internal sealed class CleanupManifest
{
    public int Version { get; set; } = 1;
    public bool Cleaned { get; set; }
    public List<CleanupInstance> Instances { get; set; } = [];
    public List<string> RemovedIds { get; set; } = [];
}

internal sealed class CleanupInstance
{
    public string Id { get; set; } = "";
    public string Label { get; set; } = "";
    public string Path { get; set; } = "";
    public string? ManagedPath { get; set; }
    public string? Origin { get; set; }
}

[JsonSourceGenerationOptions(PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase)]
[JsonSerializable(typeof(CleanupManifest))]
internal partial class CleanupJsonContext : JsonSerializerContext;
