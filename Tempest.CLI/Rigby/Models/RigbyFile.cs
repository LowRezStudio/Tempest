using System.Text.Json.Serialization;

namespace Tempest.CLI.Rigby.Models;

internal sealed class RigbyFile
{
    [JsonPropertyName("path")]
    public required string Path { get; init; }

    [JsonPropertyName("size")]
    public required long Size { get; init; }

    [JsonPropertyName("sha256")]
    public required string Sha256 { get; init; }

    [JsonPropertyName("blake3")]
    public string? Blake3 { get; init; }

    [JsonPropertyName("chunk_start")]
    public required int ChunkStart { get; init; }

    [JsonPropertyName("chunk_end")]
    public required int ChunkEnd { get; init; }

    /// <summary>Parsed once when the manifest loads, so checks compare bytes instead of building hex strings.</summary>
    [JsonIgnore]
    public byte[] Sha256Bytes { get; set; } = [];

    /// <summary>Set when the manifest lists one; BLAKE3 is what the file is checked against then.</summary>
    [JsonIgnore]
    public byte[]? Blake3Bytes { get; set; }
}
