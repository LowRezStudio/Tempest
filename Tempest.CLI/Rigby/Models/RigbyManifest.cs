using System.Text.Json;

namespace Tempest.CLI.Rigby.Models;

internal sealed class RigbyManifest
{
    public required string FormatVersion { get; init; }
    public string? PathPrefix { get; init; }
    public required List<RigbyChunk> Chunks { get; init; }
    public required List<RigbyFile> Files { get; init; }

    public static async Task<RigbyManifest> LoadAsync(string source, HttpClient http, CancellationToken cancellationToken)
    {
        await using var stream = await OpenManifestStreamAsync(source, http, cancellationToken);
        using var document = await JsonDocument.ParseAsync(stream, cancellationToken: cancellationToken);

        var root = document.RootElement;
        if (!root.TryGetProperty("format_version", out var formatVersionNode))
            throw new InvalidOperationException($"Manifest missing format_version: {source}");

        if (!root.TryGetProperty("chunks", out var chunksNode) || chunksNode.ValueKind != JsonValueKind.Array)
            throw new InvalidOperationException($"Manifest missing chunks: {source}");

        if (!root.TryGetProperty("files", out var filesNode) || filesNode.ValueKind != JsonValueKind.Array)
            throw new InvalidOperationException($"Manifest missing files: {source}");

        var chunks = new List<RigbyChunk>();
        foreach (var chunkNode in chunksNode.EnumerateArray())
        {
            chunks.Add(new RigbyChunk
            {
                Sha256 = chunkNode.GetProperty("sha256").GetString() ?? throw new InvalidOperationException($"Chunk sha256 missing in {source}"),
                Md5 = chunkNode.GetProperty("md5").GetString() ?? throw new InvalidOperationException($"Chunk md5 missing in {source}"),
                Blake3 = chunkNode.TryGetProperty("blake3", out var chunkBlake3Node) ? chunkBlake3Node.GetString() : null,
                Length = chunkNode.GetProperty("length").GetInt32(),
                Codec = chunkNode.TryGetProperty("codec", out var codecNode) ? codecNode.GetString() : null
            });
        }

        var files = new List<RigbyFile>();
        foreach (var fileNode in filesNode.EnumerateArray())
        {
            files.Add(new RigbyFile
            {
                Path = fileNode.GetProperty("path").GetString() ?? throw new InvalidOperationException($"File path missing in {source}"),
                Size = fileNode.GetProperty("size").GetInt64(),
                Sha256 = fileNode.GetProperty("sha256").GetString() ?? throw new InvalidOperationException($"File sha256 missing in {source}"),
                Md5 = fileNode.GetProperty("md5").GetString() ?? throw new InvalidOperationException($"File md5 missing in {source}"),
                Blake3 = fileNode.TryGetProperty("blake3", out var fileBlake3Node) ? fileBlake3Node.GetString() : null,
                ChunkStart = fileNode.GetProperty("chunk_start").GetInt32(),
                ChunkEnd = fileNode.GetProperty("chunk_end").GetInt32()
            });
        }

        return new RigbyManifest
        {
            FormatVersion = formatVersionNode.GetString() ?? string.Empty,
            PathPrefix = root.TryGetProperty("path_prefix", out var pathPrefixNode) ? pathPrefixNode.GetString() : null,
            Chunks = chunks,
            Files = files
        };
    }

    private static async Task<Stream> OpenManifestStreamAsync(string source, HttpClient http, CancellationToken cancellationToken)
    {
        if (Uri.TryCreate(source, UriKind.Absolute, out var uri)
            && (uri.Scheme == Uri.UriSchemeHttp || uri.Scheme == Uri.UriSchemeHttps))
            return await http.GetStreamAsync(uri, cancellationToken);

        return File.OpenRead(source);
    }
}
