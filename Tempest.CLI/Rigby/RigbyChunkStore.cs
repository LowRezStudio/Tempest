using System.Collections.Concurrent;
using System.Net;
using System.Security.Cryptography;
using Blake3;
using Tempest.CLI.Rigby.Models;
using ZstdSharp;

namespace Tempest.CLI.Rigby;

/// <summary>Where a restore reads chunk blobs from: the local chunk store, the CDN, or both.</summary>
internal readonly record struct RigbyChunkSource(string? ChunksRoot, string? BaseUrl, bool NoDownload);

internal static class RigbyChunkStore
{
    private const int DownloadAttempts = 4;

    /// <summary>Refuses a bogus Content-Length before it becomes a huge allocation (chunks are 16 MiB at most today).</summary>
    private const long MaxEncodedChunkBytes = 256 * 1024 * 1024;

    // A Decompressor owns a native zstd context; building one per chunk costs more than decoding
    // the small chunks most files are made of.
    private static readonly ConcurrentBag<Decompressor> Decompressors = [];

    public static string CodecOf(RigbyChunk chunk)
        => string.IsNullOrWhiteSpace(chunk.Codec) ? "zstd" : chunk.Codec;

    public static string RelativePath(RigbyChunk chunk)
    {
        var suffix = CodecOf(chunk) switch
        {
            "zstd" => ".zst",
            "raw" => ".raw",
            _ => ".bin"
        };

        var sha = chunk.Sha256;
        return Path.Combine(sha[..2], sha.Substring(2, 2), sha + suffix);
    }

    /// <summary>
    /// The chunk's raw bytes, decoded and checked against BLAKE3 when the manifest lists one, else
    /// SHA-256: either proves the content, and BLAKE3 is about twice as fast. MD5 is never checked,
    /// both because it is slower than both and because a chunk hash only has to catch corruption
    /// the whole-file check would catch anyway.
    /// </summary>
    public static async Task<byte[]> LoadAsync(
        RigbyChunk chunk,
        RigbyChunkSource source,
        HttpClient http,
        CancellationToken cancellationToken)
    {
        var rel = RelativePath(chunk);
        if (chunk.Length == 0)
        {
            VerifyChunk(ReadOnlySpan<byte>.Empty, chunk, rel);
            return [];
        }

        var codec = CodecOf(chunk);
        if (codec is not ("raw" or "zstd"))
            throw new InvalidOperationException($"Unsupported codec: {codec} ({rel})");

        var localPath = source.ChunksRoot is { } chunksRoot ? Path.Combine(chunksRoot, rel) : null;
        var local = localPath is not null && File.Exists(localPath);

        byte[] raw;
        if (codec == "raw")
        {
            // Read straight into the result: the chunk is never held twice in memory.
            raw = local
                ? await ReadLocalAsync(localPath!, chunk.Length, chunk.Length, cancellationToken)
                : await DownloadAsync(UrlOf(source, rel), chunk.Length, http, cancellationToken);
            if (raw.Length != chunk.Length)
                throw new InvalidOperationException($"Chunk length mismatch: got {raw.Length}, expected {chunk.Length} ({rel})");
        }
        else
        {
            var maxEncoded = Math.Min(MaxEncodedChunkBytes, Math.Max(1 << 20, chunk.Length * 2L));
            var encoded = local
                ? await ReadLocalAsync(localPath!, null, maxEncoded, cancellationToken)
                : await DownloadAsync(UrlOf(source, rel), maxEncoded, http, cancellationToken);
            raw = DecodeZstd(encoded, chunk.Length, rel);
        }

        VerifyChunk(raw, chunk, rel);
        return raw;
    }

    public static void VerifyChunk(ReadOnlySpan<byte> raw, RigbyChunk chunk, string rel)
    {
        if (!ChunkMatches(raw, chunk))
            throw new InvalidOperationException($"Chunk hash mismatch: {rel}");
    }

    public static bool ChunkMatches(ReadOnlySpan<byte> data, RigbyChunk chunk)
    {
        if (chunk.Blake3Bytes is { } blake3)
        {
            var hash = Hasher.Hash(data);
            return hash.AsSpan().SequenceEqual(blake3);
        }

        Span<byte> digest = stackalloc byte[32];
        SHA256.HashData(data, digest);
        return digest.SequenceEqual(chunk.Sha256Bytes);
    }

    private static string UrlOf(RigbyChunkSource source, string rel)
    {
        if (source.NoDownload)
            throw new InvalidOperationException($"Chunk missing locally and --no-download set: {rel}");

        if (source.BaseUrl is null)
            throw new InvalidOperationException($"Missing chunk and no --base-url provided: {rel}");

        return $"{source.BaseUrl.TrimEnd('/')}/{rel.Replace('\\', '/')}";
    }

    private static async Task<byte[]> ReadLocalAsync(
        string localPath,
        long? expectedLength,
        long maxBytes,
        CancellationToken cancellationToken)
    {
        var info = new FileInfo(localPath);
        if (info.Length > maxBytes)
            throw new InvalidOperationException($"Chunk in the local store is larger than expected: {localPath}");

        if (expectedLength is { } expected && info.Length != expected)
            throw new InvalidOperationException($"Chunk length mismatch in the local store: {localPath}");

        var blob = GC.AllocateUninitializedArray<byte>((int)info.Length);
        await using var stream = new FileStream(localPath, new FileStreamOptions
        {
            Mode = FileMode.Open,
            Access = FileAccess.Read,
            Share = FileShare.Read,
            BufferSize = 1,
            Options = FileOptions.Asynchronous | FileOptions.SequentialScan
        });
        await stream.ReadExactlyAsync(blob, cancellationToken);
        return blob;
    }

    /// <summary>Downloads one chunk, retrying transient failures; the whole body is inside the retry.</summary>
    private static async Task<byte[]> DownloadAsync(
        string url,
        long maxBytes,
        HttpClient http,
        CancellationToken cancellationToken)
    {
        for (var attempt = 1; ; attempt++)
        {
            try
            {
                return await DownloadOnceAsync(url, maxBytes, http, cancellationToken);
            }
            catch (Exception ex) when (IsTransient(ex) && !cancellationToken.IsCancellationRequested)
            {
                if (attempt >= DownloadAttempts)
                    throw new InvalidOperationException($"Failed to download chunk after {attempt} attempts: {url}", ex);

                await Task.Delay(TimeSpan.FromSeconds(1 << (attempt - 1)), cancellationToken);
            }
        }
    }

    private static bool IsTransient(Exception exception) => exception switch
    {
        HttpRequestException { StatusCode: { } status } =>
            status is HttpStatusCode.RequestTimeout or HttpStatusCode.TooManyRequests || (int)status >= 500,
        HttpRequestException => true, // connection reset, DNS failure, truncated response
        IOException => true,
        OperationCanceledException => true, // HttpClient timeout; a caller cancellation is filtered out above
        _ => false
    };

    private static async Task<byte[]> DownloadOnceAsync(
        string url,
        long maxBytes,
        HttpClient http,
        CancellationToken cancellationToken)
    {
        using var response = await http.GetAsync(url, HttpCompletionOption.ResponseHeadersRead, cancellationToken);
        if (!response.IsSuccessStatusCode)
            throw new HttpRequestException($"Chunk download failed ({(int)response.StatusCode}): {url}", null, response.StatusCode);

        var content = response.Content;
        if (content.Headers.ContentLength is { } length)
        {
            if (length > maxBytes)
                throw new InvalidOperationException($"Downloaded chunk is larger than expected: {url} ({length} bytes)");

            var blob = GC.AllocateUninitializedArray<byte>((int)length);
            await using var body = await content.ReadAsStreamAsync(cancellationToken);
            await body.ReadExactlyAsync(blob, cancellationToken);
            return blob;
        }

        // No Content-Length (a proxy may drop it): grow while reading and stop at the bound.
        await using (var body = await content.ReadAsStreamAsync(cancellationToken))
        {
            var buffer = new MemoryStream((int)Math.Min(maxBytes, 1 << 20));
            await body.CopyToAsync(buffer, cancellationToken);
            if (buffer.Length > maxBytes)
                throw new InvalidOperationException($"Downloaded chunk is larger than expected: {url} ({buffer.Length} bytes)");

            return buffer.ToArray();
        }
    }

    private static byte[] DecodeZstd(ReadOnlySpan<byte> encoded, int expectedLength, string rel)
    {
        var decompressor = Decompressors.TryTake(out var pooled) ? pooled : new Decompressor();
        var output = GC.AllocateUninitializedArray<byte>(expectedLength);
        try
        {
            var written = decompressor.Unwrap(encoded, output);
            if (written != expectedLength)
                throw new InvalidOperationException($"Decoded chunk length mismatch (zstd): got {written}, expected {expectedLength} ({rel})");
        }
        catch
        {
            // Don't pool a context that failed mid-frame.
            decompressor.Dispose();
            throw;
        }

        Decompressors.Add(decompressor);
        return output;
    }
}
