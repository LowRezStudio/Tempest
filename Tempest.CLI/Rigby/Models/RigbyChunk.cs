using System.Text.Json.Serialization;

namespace Tempest.CLI.Rigby.Models;

internal sealed class RigbyChunk
{
    [JsonPropertyName("sha256")]
    public required string Sha256 { get; init; }

    [JsonPropertyName("blake3")]
    public string? Blake3 { get; init; }

    [JsonPropertyName("length")]
    public required int Length { get; init; }

    [JsonPropertyName("codec")]
    public string? Codec { get; init; }

    /// <summary>Parsed once when the manifest loads, so checks compare bytes instead of building hex strings.</summary>
    [JsonIgnore]
    public byte[] Sha256Bytes { get; set; } = [];

    /// <summary>Set when the manifest lists one; BLAKE3 is what a chunk is checked against then.</summary>
    [JsonIgnore]
    public byte[]? Blake3Bytes { get; set; }
}
