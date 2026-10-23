using System.Buffers;
using Microsoft.Win32.SafeHandles;
using Tempest.CLI.Rigby.Models;

namespace Tempest.CLI.Rigby;

internal static class RigbyRestoreEngine
{
    private const int HashBufferSize = 1024 * 1024;

    /// <summary>
    /// How much of the file an interrupted run already wrote to its temp copy, in whole chunks.
    /// Every chunk was verified before it was written, so a resumed run can keep them unread.
    /// </summary>
    public static long ResumablePrefix(RestoreTask task)
    {
        var tempPath = TempPathOf(task);
        if (!File.Exists(tempPath))
            return 0;

        var length = new FileInfo(tempPath).Length;
        return length > TotalSize(task) ? 0 : WholeChunks(task, length).Bytes;
    }

    public static async Task<RestoreResult> RestoreOneAsync(
        RestoreTask task,
        long resumedPrefix,
        RigbyChunkSource source,
        HttpClient http,
        IRestoreProgress progress,
        CancellationToken cancellationToken)
    {
        if (resumedPrefix > 0)
            return await ContinueFileAsync(task, resumedPrefix, source, http, progress, cancellationToken);

        // One stat decides both "already there and right" and "there but damaged".
        var existing = new FileInfo(task.OutputPath);
        if (existing.Exists && existing.Length == task.File.Size)
        {
            if (await FileMatchesAsync(task.OutputPath, task.File, cancellationToken))
            {
                progress.BytesReused(task.File.Size);
                return new RestoreResult(false, 0, task.File.Size);
            }

            var patchedBytes = await PatchMismatchedChunksAsync(task, source, http, progress, cancellationToken);
            if (await FileMatchesAsync(task.OutputPath, task.File, cancellationToken))
                return new RestoreResult(true, patchedBytes, Math.Max(0, task.File.Size - patchedBytes));
        }

        return await RebuildFileAsync(task, source, http, progress, cancellationToken);
    }

    /// <summary>
    /// Appends the rest of a file to the prefix an interrupted run left, which the caller already
    /// counted, without reading that prefix back first; the whole file is checked once at the end.
    /// </summary>
    private static async Task<RestoreResult> ContinueFileAsync(
        RestoreTask task,
        long prefix,
        RigbyChunkSource source,
        HttpClient http,
        IRestoreProgress progress,
        CancellationToken cancellationToken)
    {
        var tempPath = TempPathOf(task);
        long newBytesWritten = 0;

        await using (var output = OpenWrite(tempPath, FileMode.Open))
        {
            // Drops a chunk the interruption cut short.
            output.SetLength(prefix);
            output.Position = prefix;

            for (var i = WholeChunks(task, prefix).NextChunk; i < task.File.ChunkEnd; i++)
            {
                var raw = await FetchChunkAsync(task, i, source, http, cancellationToken);
                await output.WriteAsync(raw, cancellationToken);
                newBytesWritten += raw.Length;
                progress.BytesWritten(raw.Length);
            }
        }

        if (!await FileMatchesAsync(tempPath, task.File, cancellationToken))
        {
            // Without the temp copy, the next run builds this file from nothing.
            File.Delete(tempPath);
            throw new InvalidOperationException($"File hash mismatch after resuming: {task.OutputPath}");
        }

        File.Move(tempPath, task.OutputPath, true);
        return new RestoreResult(true, newBytesWritten, prefix);
    }

    private static async Task<byte[]> FetchChunkAsync(
        RestoreTask task,
        int index,
        RigbyChunkSource source,
        HttpClient http,
        CancellationToken cancellationToken)
    {
        if (index < 0 || index >= task.Chunks.Count)
            throw new InvalidOperationException($"Invalid chunk range for file: {task.File.Path}");

        return await RigbyChunkStore.LoadAsync(task.Chunks[index], source, http, cancellationToken);
    }

    private static string TempPathOf(RestoreTask task) => task.OutputPath + ".rigby.tmp";

    private static long TotalSize(RestoreTask task)
    {
        var totalSize = 0L;
        for (var i = task.File.ChunkStart; i < task.File.ChunkEnd; i++)
            totalSize += task.Chunks[i].Length;

        return totalSize;
    }

    /// <summary>The longest run of whole chunks that fits in <paramref name="length"/> bytes.</summary>
    private static (long Bytes, int NextChunk) WholeChunks(RestoreTask task, long length)
    {
        var bytes = 0L;
        var nextChunk = task.File.ChunkStart;
        for (var i = task.File.ChunkStart; i < task.File.ChunkEnd; i++)
        {
            var chunkLen = task.Chunks[i].Length;
            if (bytes + chunkLen > length)
                break;

            bytes += chunkLen;
            nextChunk = i + 1;
        }

        return (bytes, nextChunk);
    }

    private static async Task<RestoreResult> RebuildFileAsync(
        RestoreTask task,
        RigbyChunkSource source,
        HttpClient http,
        IRestoreProgress progress,
        CancellationToken cancellationToken)
    {
        RigbyOutputLayout.EnsureParent(task.OutputPath);
        var tempPath = TempPathOf(task);
        var totalSize = TotalSize(task);

        var temp = new FileInfo(tempPath);
        if (temp.Exists && temp.Length == totalSize)
        {
            if (await FileMatchesAsync(tempPath, task.File, cancellationToken))
            {
                File.Move(tempPath, task.OutputPath, true);
                progress.BytesReused(task.File.Size);
                return new RestoreResult(false, 0, task.File.Size);
            }

            File.Delete(tempPath);
            temp = new FileInfo(tempPath);
        }

        var existingBytes = 0L;
        var startChunkIndex = task.File.ChunkStart;
        if (temp.Exists)
        {
            if (temp.Length > totalSize)
            {
                File.Delete(tempPath);
            }
            else
            {
                (existingBytes, startChunkIndex) = WholeChunks(task, temp.Length);
            }
        }

        await using var output = OpenWrite(tempPath, FileMode.OpenOrCreate);

        // Drops a chunk an earlier run cut short; no seek, the stream is already at the end.
        if (existingBytes < output.Length)
            output.SetLength(existingBytes);

        using var digests = new RigbyFileDigests(task.File);
        long written = 0;
        long newBytesWritten = 0;

        if (existingBytes > 0)
        {
            output.Position = 0;
            await HashPrefixAsync(output, existingBytes, digests, cancellationToken);
            output.Position = existingBytes;

            written = existingBytes;
            progress.BytesReused(existingBytes);
        }

        for (var i = startChunkIndex; i < task.File.ChunkEnd; i++)
        {
            var raw = await FetchChunkAsync(task, i, source, http, cancellationToken);

            await output.WriteAsync(raw, cancellationToken);
            digests.Append(raw);
            written += raw.Length;
            newBytesWritten += raw.Length;
            progress.BytesWritten(raw.Length);
        }

        if (written != task.File.Size)
            throw new InvalidOperationException($"File size mismatch: {task.OutputPath}");

        // Hashes were computed while the chunks were in memory, so the file is never read back.
        var mismatch = digests.Mismatch(task.File);
        if (mismatch is not null)
            throw new InvalidOperationException($"File {mismatch} mismatch: {task.OutputPath}");

        await output.FlushAsync(cancellationToken);
        output.Close();
        File.Move(tempPath, task.OutputPath, true);

        return new RestoreResult(true, newBytesWritten, existingBytes);
    }

    private static async Task<long> PatchMismatchedChunksAsync(
        RestoreTask task,
        RigbyChunkSource source,
        HttpClient http,
        IRestoreProgress progress,
        CancellationToken cancellationToken)
    {
        using var handle = File.OpenHandle(
            task.OutputPath,
            FileMode.Open,
            FileAccess.ReadWrite,
            FileShare.None,
            FileOptions.Asynchronous | FileOptions.SequentialScan);
        var buffer = ArrayPool<byte>.Shared.Rent(MaxChunkLength(task));
        try
        {
            long patchedBytes = 0;
            long position = 0;

            for (var i = task.File.ChunkStart; i < task.File.ChunkEnd; i++)
            {
                if (i < 0 || i >= task.Chunks.Count)
                    throw new InvalidOperationException($"Invalid chunk range for file: {task.File.Path}");

                var chunk = task.Chunks[i];
                var existing = buffer.AsMemory(0, chunk.Length);
                await ReadExactlyAsync(handle, existing, position, task.OutputPath, cancellationToken);
                if (RigbyChunkStore.ChunkMatches(existing.Span, chunk))
                {
                    position += chunk.Length;
                    progress.BytesReused(chunk.Length);
                    continue;
                }

                var raw = await FetchChunkAsync(task, i, source, http, cancellationToken);

                await RandomAccess.WriteAsync(handle, raw, position, cancellationToken);
                patchedBytes += raw.Length;
                position += raw.Length;
                progress.BytesWritten(raw.Length);
            }

            return patchedBytes;
        }
        finally
        {
            ArrayPool<byte>.Shared.Return(buffer);
        }
    }

    private static int MaxChunkLength(RestoreTask task)
    {
        var max = 1;
        for (var i = task.File.ChunkStart; i < task.File.ChunkEnd; i++)
            max = Math.Max(max, task.Chunks[i].Length);

        return max;
    }

    private static async Task<bool> FileMatchesAsync(string path, RigbyFile file, CancellationToken cancellationToken)
    {
        var info = new FileInfo(path);
        if (!info.Exists || info.Length != file.Size)
            return false;

        await using var stream = new FileStream(path, new FileStreamOptions
        {
            Mode = FileMode.Open,
            Access = FileAccess.Read,
            Share = FileShare.Read,
            BufferSize = 1,
            Options = FileOptions.Asynchronous | FileOptions.SequentialScan
        });

        using var digests = new RigbyFileDigests(file);
        var buffer = ArrayPool<byte>.Shared.Rent(HashBufferSize);
        try
        {
            while (true)
            {
                var read = await stream.ReadAsync(buffer.AsMemory(0, buffer.Length), cancellationToken);
                if (read == 0)
                    break;

                digests.Append(buffer.AsSpan(0, read));
            }
        }
        finally
        {
            ArrayPool<byte>.Shared.Return(buffer);
        }

        return digests.Mismatch(file) is null;
    }

    /// <summary>Feeds the part of a temp file an interrupted run wrote into the file's digests.</summary>
    private static async Task HashPrefixAsync(
        FileStream stream,
        long length,
        RigbyFileDigests digests,
        CancellationToken cancellationToken)
    {
        var buffer = ArrayPool<byte>.Shared.Rent(HashBufferSize);
        try
        {
            var remaining = length;
            while (remaining > 0)
            {
                var read = await stream.ReadAsync(buffer.AsMemory(0, (int)Math.Min(buffer.Length, remaining)), cancellationToken);
                if (read == 0)
                    throw new InvalidOperationException($"Unexpected end of temp file: {stream.Name}");

                digests.Append(buffer.AsSpan(0, read));
                remaining -= read;
            }
        }
        finally
        {
            ArrayPool<byte>.Shared.Return(buffer);
        }
    }

    private static async Task ReadExactlyAsync(
        SafeFileHandle handle,
        Memory<byte> buffer,
        long position,
        string path,
        CancellationToken cancellationToken)
    {
        while (!buffer.IsEmpty)
        {
            var read = await RandomAccess.ReadAsync(handle, buffer, position, cancellationToken);
            if (read == 0)
                throw new InvalidOperationException($"Unexpected end of file while reading existing data: {path}");

            buffer = buffer[read..];
            position += read;
        }
    }

    /// <summary>Async, unbuffered and sequential: the engine reads and writes whole chunks itself.</summary>
    private static FileStream OpenWrite(string path, FileMode mode) => new(path, new FileStreamOptions
    {
        Mode = mode,
        Access = FileAccess.ReadWrite,
        Share = FileShare.None,
        BufferSize = 1,
        Options = FileOptions.Asynchronous | FileOptions.SequentialScan
    });
}
