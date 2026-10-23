using System.Text.Json;
using System.Text.Json.Serialization;

namespace Tempest.CLI.Rigby.Models;

internal sealed class RigbyManifest
{
    [JsonPropertyName("format_version")]
    public required string FormatVersion { get; init; }

    [JsonPropertyName("path_prefix")]
    public string? PathPrefix { get; init; }

    [JsonPropertyName("chunks")]
    public required List<RigbyChunk> Chunks { get; init; }

    [JsonPropertyName("files")]
    public required List<RigbyFile> Files { get; init; }

    public static async Task<RigbyManifest> LoadAsync(string source, HttpClient http, CancellationToken cancellationToken)
    {
        var json = await ReadManifestBytesAsync(source, http, cancellationToken);

        RigbyManifest? manifest;
        try
        {
            // Source-generated over the bytes we already hold: no JsonDocument tree of every node.
            manifest = JsonSerializer.Deserialize(json, RigbyManifestJsonContext.Default.RigbyManifest);
        }
        catch (JsonException ex)
        {
            throw new InvalidOperationException($"Manifest is not valid JSON ({source}): {ex.Message}", ex);
        }

        if (manifest is null)
            throw new InvalidOperationException($"Manifest is empty: {source}");

        manifest.Prepare(source);
        return manifest;
    }

    /// <summary>
    /// Parses the hex digests once, so a restore compares bytes instead of formatting a hash per
    /// chunk and lower-casing it.
    /// </summary>
    private void Prepare(string source)
    {
        for (var i = 0; i < Chunks.Count; i++)
        {
            var chunk = Chunks[i];
            chunk.Sha256Bytes = ParseDigest(chunk.Sha256, 32, "chunk sha256", source);
            chunk.Blake3Bytes = string.IsNullOrWhiteSpace(chunk.Blake3)
                ? null
                : ParseDigest(chunk.Blake3, 32, "chunk blake3", source);
        }

        for (var i = 0; i < Files.Count; i++)
        {
            var file = Files[i];
            file.Sha256Bytes = ParseDigest(file.Sha256, 32, "file sha256", source);
            file.Blake3Bytes = string.IsNullOrWhiteSpace(file.Blake3)
                ? null
                : ParseDigest(file.Blake3, 32, "file blake3", source);
        }
    }

    private static byte[] ParseDigest(string? hex, int bytes, string what, string source)
    {
        if (string.IsNullOrWhiteSpace(hex) || hex.Length != bytes * 2)
            throw new InvalidOperationException($"Manifest {what} is not a {bytes}-byte hex digest: {source}");

        try
        {
            return Convert.FromHexString(hex);
        }
        catch (FormatException ex)
        {
            throw new InvalidOperationException($"Manifest {what} is not hex: {source}", ex);
        }
    }

    private static async Task<byte[]> ReadManifestBytesAsync(string source, HttpClient http, CancellationToken cancellationToken)
    {
        if (Uri.TryCreate(source, UriKind.Absolute, out var uri)
            && (uri.Scheme == Uri.UriSchemeHttp || uri.Scheme == Uri.UriSchemeHttps))
            return await http.GetByteArrayAsync(uri, cancellationToken);

        return await File.ReadAllBytesAsync(source, cancellationToken);
    }
}

[JsonSerializable(typeof(RigbyManifest))]
internal partial class RigbyManifestJsonContext : JsonSerializerContext
{
}
