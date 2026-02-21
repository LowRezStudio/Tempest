using System.Buffers;
using System.Security.Cryptography;
using Blake3;
using Tempest.CLI.Rigby.Models;

namespace Tempest.CLI.Rigby;

internal static class RigbyRestoreEngine
{
    public static async Task<RestoreResult> RestoreOneAsync(
        RestoreTask task,
        string? chunksRoot,
        string? baseUrl,
        bool noDownload,
        HttpClient http,
        CancellationToken cancellationToken)
    {
        if (await IsFileValidAsync(task.OutputPath, task.File, cancellationToken))
            return new RestoreResult(false, 0, task.File.Size);

        if (File.Exists(task.OutputPath) && new FileInfo(task.OutputPath).Length == task.File.Size)
        {
            var patchedBytes = await PatchMismatchedChunksAsync(task, chunksRoot, baseUrl, noDownload, http, cancellationToken);
            if (await IsFileValidAsync(task.OutputPath, task.File, cancellationToken))
                return new RestoreResult(true, patchedBytes, Math.Max(0, task.File.Size - patchedBytes));
        }

        await RebuildFileAsync(task, chunksRoot, baseUrl, noDownload, http, cancellationToken);
        return new RestoreResult(true, task.File.Size, 0);
    }

    private static async Task RebuildFileAsync(
        RestoreTask task,
        string? chunksRoot,
        string? baseUrl,
        bool noDownload,
        HttpClient http,
        CancellationToken cancellationToken)
    {
        RigbyOutputLayout.EnsureParent(task.OutputPath);
        var tempPath = task.OutputPath + ".rigby.tmp";

        await using var output = new FileStream(tempPath, FileMode.Create, FileAccess.Write, FileShare.None);
        using var fileSha256 = SHA256.Create();
        using var fileMd5 = MD5.Create();
        var fileBlake3 = Hasher.New();

        long written = 0;

        for (var i = task.File.ChunkStart; i < task.File.ChunkEnd; i++)
        {
            if (i < 0 || i >= task.Chunks.Count)
                throw new InvalidOperationException($"Invalid chunk range for file: {task.File.Path}");

            var chunk = task.Chunks[i];
            var codec = string.IsNullOrWhiteSpace(chunk.Codec) ? "zstd" : chunk.Codec;
            var rel = RigbyChunkStore.ChunkRelativePath(chunk.Sha256, codec);

            var encoded = await RigbyChunkStore.GetEncodedChunkAsync(rel, chunksRoot, baseUrl, noDownload, http, cancellationToken);
            var raw = RigbyChunkStore.DecodeChunk(encoded, codec, chunk.Length);
            RigbyChunkStore.VerifyChunk(raw, chunk, rel);

            await output.WriteAsync(raw, cancellationToken);
            fileSha256.TransformBlock(raw, 0, raw.Length, null, 0);
            fileMd5.TransformBlock(raw, 0, raw.Length, null, 0);
            fileBlake3.Update(raw);
            written += raw.Length;
        }

        fileSha256.TransformFinalBlock([], 0, 0);
        fileMd5.TransformFinalBlock([], 0, 0);

        var outputSha256 = Convert.ToHexString(fileSha256.Hash!).ToLowerInvariant();
        var outputMd5 = Convert.ToHexString(fileMd5.Hash!).ToLowerInvariant();
        var outputBlake3 = fileBlake3.Finalize().ToString();

        if (written != task.File.Size)
            throw new InvalidOperationException($"File size mismatch: {task.OutputPath}");

        if (!string.Equals(outputSha256, task.File.Sha256, StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException($"File sha256 mismatch: {task.OutputPath}");

        if (!string.Equals(outputMd5, task.File.Md5, StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException($"File md5 mismatch: {task.OutputPath}");

        if (!string.IsNullOrWhiteSpace(task.File.Blake3)
            && !string.Equals(outputBlake3, task.File.Blake3, StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException($"File blake3 mismatch: {task.OutputPath}");

        await output.FlushAsync(cancellationToken);
        output.Close();
        File.Move(tempPath, task.OutputPath, true);
    }

    private static async Task<long> PatchMismatchedChunksAsync(
        RestoreTask task,
        string? chunksRoot,
        string? baseUrl,
        bool noDownload,
        HttpClient http,
        CancellationToken cancellationToken)
    {
        await using var stream = new FileStream(task.OutputPath, FileMode.Open, FileAccess.ReadWrite, FileShare.None);

        long patchedBytes = 0;
        long position = 0;

        for (var i = task.File.ChunkStart; i < task.File.ChunkEnd; i++)
        {
            if (i < 0 || i >= task.Chunks.Count)
                throw new InvalidOperationException($"Invalid chunk range for file: {task.File.Path}");

            var chunk = task.Chunks[i];
            var existing = await ReadExactlyAsync(stream, position, chunk.Length, cancellationToken);
            if (RigbyChunkStore.ChunkMatches(existing, chunk))
            {
                position += chunk.Length;
                continue;
            }

            var codec = string.IsNullOrWhiteSpace(chunk.Codec) ? "zstd" : chunk.Codec;
            var rel = RigbyChunkStore.ChunkRelativePath(chunk.Sha256, codec);
            var encoded = await RigbyChunkStore.GetEncodedChunkAsync(rel, chunksRoot, baseUrl, noDownload, http, cancellationToken);
            var raw = RigbyChunkStore.DecodeChunk(encoded, codec, chunk.Length);
            RigbyChunkStore.VerifyChunk(raw, chunk, rel);

            stream.Position = position;
            await stream.WriteAsync(raw, cancellationToken);
            patchedBytes += raw.Length;
            position += raw.Length;
        }

        await stream.FlushAsync(cancellationToken);
        return patchedBytes;
    }

    private static async Task<byte[]> ReadExactlyAsync(FileStream stream, long position, int length, CancellationToken cancellationToken)
    {
        var buffer = new byte[length];
        stream.Position = position;

        var readTotal = 0;
        while (readTotal < length)
        {
            var read = await stream.ReadAsync(buffer.AsMemory(readTotal, length - readTotal), cancellationToken);
            if (read == 0)
                throw new InvalidOperationException($"Unexpected end of file while reading existing data: {stream.Name}");

            readTotal += read;
        }

        return buffer;
    }

    private static async Task<bool> IsFileValidAsync(string path, RigbyFile file, CancellationToken cancellationToken)
    {
        if (!File.Exists(path))
            return false;

        var info = new FileInfo(path);
        if (info.Length != file.Size)
            return false;

        await using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
        using var sha256 = SHA256.Create();
        using var md5 = MD5.Create();
        var blake3 = Hasher.New();

        var buffer = ArrayPool<byte>.Shared.Rent(1024 * 1024);
        try
        {
            while (true)
            {
                var read = await stream.ReadAsync(buffer.AsMemory(0, buffer.Length), cancellationToken);
                if (read == 0)
                    break;

                sha256.TransformBlock(buffer, 0, read, null, 0);
                md5.TransformBlock(buffer, 0, read, null, 0);

                blake3.Update(buffer.AsSpan(0, read));
            }
        }
        finally
        {
            ArrayPool<byte>.Shared.Return(buffer);
        }

        sha256.TransformFinalBlock([], 0, 0);
        md5.TransformFinalBlock([], 0, 0);

        var existingSha256 = Convert.ToHexString(sha256.Hash!).ToLowerInvariant();
        var existingMd5 = Convert.ToHexString(md5.Hash!).ToLowerInvariant();
        var existingBlake3 = blake3.Finalize().ToString();

        if (!string.Equals(existingSha256, file.Sha256, StringComparison.OrdinalIgnoreCase))
            return false;

        if (!string.Equals(existingMd5, file.Md5, StringComparison.OrdinalIgnoreCase))
            return false;

        if (!string.IsNullOrWhiteSpace(file.Blake3)
            && !string.Equals(existingBlake3, file.Blake3, StringComparison.OrdinalIgnoreCase))
            return false;

        return true;
    }
}
