using System.Text.Json.Serialization;

namespace Tempest.CLI.Rigby.Events;

[method: JsonConstructor]
public sealed record RigbyProgressEvent(
    [property: JsonPropertyName("type")] string Type,
    [property: JsonPropertyName("completedFiles")] int CompletedFiles,
    [property: JsonPropertyName("totalFiles")] int TotalFiles,
    [property: JsonPropertyName("percent")] double Percent,
    [property: JsonPropertyName("completedBytes")] long CompletedBytes,
    [property: JsonPropertyName("totalBytes")] long TotalBytes,
    [property: JsonPropertyName("bytesPerSecond")] double BytesPerSecond,
    [property: JsonPropertyName("etaSeconds")] double EtaSeconds,
    [property: JsonPropertyName("repairedFiles")] int RepairedFiles,
    [property: JsonPropertyName("verifiedFiles")] int VerifiedFiles,
    [property: JsonPropertyName("diskWriteBytes")] long DiskWriteBytes,
    [property: JsonPropertyName("reusedBytes")] long ReusedBytes
);