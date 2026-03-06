using System.Buffers;
using System.Security.Cryptography;
using Blake3;
using Tempest.CLI.Rigby.Models;
using ZstdSharp;

namespace Tempest.CLI.Rigby;

internal static class RigbyChunkStore
{
    public static async Task<byte[]> GetEncodedChunkAsync(
        string rel,
        string? chunksRoot,
        string? baseUrl,
        bool noDownload,
        HttpClient http,
        CancellationToken cancellationToken)
    {
        if (chunksRoot is not null)
        {
            var localPath = Path.Combine(chunksRoot, rel);
            if (File.Exists(localPath))
                return await File.ReadAllBytesAsync(localPath, cancellationToken);
        }

        if (noDownload)
            throw new InvalidOperationException($"Chunk missing locally and --no-download set: {rel}");

        if (baseUrl is null)
            throw new InvalidOperationException($"Missing chunk and no --base-url provided: {rel}");

        var url = $"{baseUrl.TrimEnd('/')}/{rel.Replace('\\', '/')}";
        try
        {
            return await http.GetByteArrayAsync(url, cancellationToken);
        }
        catch (HttpRequestException ex)
        {
            throw new InvalidOperationException($"Failed to download chunk: {url}", ex);
        }
    }

    public static byte[] DecodeChunk(byte[] blob, string codec, int expectedLength)
    {
        return codec switch
        {
            "raw" => ValidateLength(blob, expectedLength, codec),
            "zstd" => DecodeZstd(blob, expectedLength),
            _ => throw new InvalidOperationException($"Unsupported codec: {codec}")
        };
    }

    public static string ChunkRelativePath(string chunkSha256, string codec)
    {
        var suffix = codec switch
        {
            "zstd" => ".zst",
            "raw" => ".raw",
            _ => ".bin"
        };

        return Path.Combine(chunkSha256[..2], chunkSha256.Substring(2, 2), chunkSha256 + suffix);
    }

    public static void VerifyChunk(byte[] raw, RigbyChunk chunk, string rel)
    {
        if (!ChunkMatches(raw, chunk))
            throw new InvalidOperationException($"Chunk hash mismatch: {rel}");
    }

    public static bool ChunkMatches(byte[] data, RigbyChunk chunk)
    {
        var sha = Convert.ToHexString(SHA256.HashData(data)).ToLowerInvariant();
        if (!string.Equals(sha, chunk.Sha256, StringComparison.OrdinalIgnoreCase))
            return false;

        var md5 = Convert.ToHexString(MD5.HashData(data)).ToLowerInvariant();
        if (!string.Equals(md5, chunk.Md5, StringComparison.OrdinalIgnoreCase))
            return false;

        if (!string.IsNullOrWhiteSpace(chunk.Blake3))
        {
            var blake3 = Hasher.Hash(data).ToString();
            if (!string.Equals(blake3, chunk.Blake3, StringComparison.OrdinalIgnoreCase))
                return false;
        }

        return true;
    }

    private static byte[] ValidateLength(byte[] data, int expectedLength, string codec)
    {
        if (data.Length != expectedLength)
            throw new InvalidOperationException($"Decoded chunk length mismatch ({codec}): got {data.Length}, expected {expectedLength}");

        return data;
    }

    private static byte[] DecodeZstd(byte[] blob, int expectedLength)
    {
        using var decompressor = new Decompressor();
        var rented = ArrayPool<byte>.Shared.Rent(expectedLength);
        try
        {
            var written = decompressor.Unwrap(blob, rented, 0);
            if (written != expectedLength)
                throw new InvalidOperationException($"Decoded chunk length mismatch (zstd): got {written}, expected {expectedLength}");

            var output = new byte[written];
            Buffer.BlockCopy(rented, 0, output, 0, written);
            return output;
        }
        finally
        {
            ArrayPool<byte>.Shared.Return(rented);
        }
    }
}
