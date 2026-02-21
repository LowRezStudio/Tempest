using System.Buffers;
using System.Diagnostics;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.RegularExpressions;
using Blake3;
using ConsoleAppFramework;
using ZstdSharp;

namespace Tempest.CLI.Rigby;

internal sealed class RigbyCommands
{
    private const string FormatVersion = "rigby-v1";

    public async Task Restore(
        [Argument] string[] manifests,
        string outDir,
        string? chunksRoot = null,
        string? baseUrl = null,
        bool json = false,
        bool noDownload = false,
        CancellationToken cancellationToken = default)
    {
        if (manifests.Length == 0)
            throw new InvalidOperationException("At least one manifest path or glob is required.");

        if (string.IsNullOrWhiteSpace(chunksRoot) && string.IsNullOrWhiteSpace(baseUrl))
            throw new InvalidOperationException("Either --chunks-root or --base-url is required.");

        var workers = Math.Max(1, Environment.ProcessorCount);

        var manifestPaths = ExpandManifestInputs(manifests);
        if (manifestPaths.Count == 0)
            throw new InvalidOperationException("No manifest files matched the provided inputs.");

        var outRoot = Path.GetFullPath(outDir);
        Directory.CreateDirectory(outRoot);

        var chunksRootFull = string.IsNullOrWhiteSpace(chunksRoot) ? null : Path.GetFullPath(chunksRoot);
        if (chunksRootFull is not null && !Directory.Exists(chunksRootFull))
            throw new InvalidOperationException($"Chunks root does not exist: {chunksRootFull}");

        using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(30.0) };
        var tasks = new List<RestoreTask>();
        var expectedFiles = new HashSet<string>(GetPathComparer());

        foreach (var manifestPath in manifestPaths)
        {
            var manifest = await RigbyManifest.LoadAsync(manifestPath, http, cancellationToken);

            if (!string.Equals(manifest.FormatVersion, FormatVersion, StringComparison.Ordinal))
                throw new InvalidOperationException($"Unsupported manifest format in {manifestPath}: {manifest.FormatVersion}");

            var manifestPrefix = NormalizePrefix(manifest.PathPrefix);
            var prefix = ResolveEffectivePrefix(outRoot, manifestPrefix, manifest.Files);

            foreach (var file in manifest.Files)
            {
                var relPath = string.IsNullOrEmpty(prefix)
                    ? file.Path
                    : Path.Combine(prefix, file.Path);

                var outputPath = Path.GetFullPath(Path.Combine(outRoot, relPath));
                if (!IsPathWithinRoot(outRoot, outputPath))
                    throw new InvalidOperationException($"Manifest path escapes output directory: {relPath}");

                tasks.Add(new RestoreTask(outputPath, relPath, file, manifest.Chunks));
                expectedFiles.Add(outputPath);
            }
        }

        var total = tasks.Count;
        var totalBytes = tasks.Sum(static x => x.File.Size);
        var repairedFiles = 0;
        var verifiedFiles = 0;
        long diskWriteBytes = 0;
        long reusedBytes = 0;

        if (total > 0)
        {
            using IRestoreProgress progress = json
                ? new JsonRestoreProgress(total, totalBytes, outRoot)
                : new RestoreProgress(total, totalBytes);

            await Parallel.ForEachAsync(
                tasks,
                new ParallelOptions
                {
                    MaxDegreeOfParallelism = workers,
                    CancellationToken = cancellationToken
                },
                async (task, ct) =>
                {
                    progress.FileStarted(task.DisplayPath);
                    try
                    {
                        var result = await RestoreOneAsync(task, chunksRootFull, baseUrl, noDownload, http, ct);
                        if (result.Repaired)
                        {
                            Interlocked.Increment(ref repairedFiles);
                        }
                        else
                        {
                            Interlocked.Increment(ref verifiedFiles);
                        }

                        Interlocked.Add(ref diskWriteBytes, result.DiskWriteBytes);
                        Interlocked.Add(ref reusedBytes, result.ReusedBytes);

                        progress.FileCompleted(result);
                    }
                    finally
                    {
                        progress.FileEnded(task.DisplayPath);
                    }
                });
        }

        var deletedFiles = DeleteUnexpectedFiles(outRoot, expectedFiles);

        if (json)
        {
            Console.WriteLine(JsonSerializer.Serialize(
                new RigbyCompleteEvent(
                    "complete",
                    total,
                    outRoot,
                    repairedFiles,
                    verifiedFiles,
                    deletedFiles,
                    diskWriteBytes,
                    reusedBytes),
                RigbyJsonOutputContext.Default.RigbyCompleteEvent));
        }
        else
        {
            Console.WriteLine(
                $"Restore complete. files={total}, repaired={repairedFiles}, verified={verifiedFiles}, deleted={deletedFiles}, " +
                $"disk_write={FormatMiB(diskWriteBytes)}, reused={FormatMiB(reusedBytes)}, out={outRoot}");
        }
    }

    private static string FormatMiB(long bytes)
    {
        var mib = bytes / 1024.0 / 1024.0;
        return $"{mib:0.0}MiB";
    }

    private static async Task<RestoreResult> RestoreOneAsync(
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
        EnsureParent(task.OutputPath);
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
            var rel = ChunkRelativePath(chunk.Sha256, codec);

            var encoded = await GetEncodedChunkAsync(rel, chunksRoot, baseUrl, noDownload, http, cancellationToken);
            var raw = DecodeChunk(encoded, codec, chunk.Length);

            VerifyChunk(raw, chunk, rel);

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
            if (ChunkMatches(existing, chunk))
            {
                position += chunk.Length;
                continue;
            }

            var codec = string.IsNullOrWhiteSpace(chunk.Codec) ? "zstd" : chunk.Codec;
            var rel = ChunkRelativePath(chunk.Sha256, codec);
            var encoded = await GetEncodedChunkAsync(rel, chunksRoot, baseUrl, noDownload, http, cancellationToken);
            var raw = DecodeChunk(encoded, codec, chunk.Length);
            VerifyChunk(raw, chunk, rel);

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

                var chunk = new byte[read];
                Buffer.BlockCopy(buffer, 0, chunk, 0, read);
                blake3.Update(chunk);
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

    private static void VerifyChunk(byte[] raw, RigbyChunk chunk, string rel)
    {
        if (!ChunkMatches(raw, chunk))
            throw new InvalidOperationException($"Chunk hash mismatch: {rel}");
    }

    private static bool ChunkMatches(byte[] data, RigbyChunk chunk)
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

    private static async Task<byte[]> GetEncodedChunkAsync(
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

        byte[] bytes;
        try
        {
            bytes = await http.GetByteArrayAsync(url, cancellationToken);
        }
        catch (HttpRequestException ex)
        {
            throw new InvalidOperationException($"Failed to download chunk: {url}", ex);
        }

        return bytes;
    }

    private static byte[] DecodeChunk(byte[] blob, string codec, int expectedLength)
    {
        return codec switch
        {
            "raw" => ValidateLength(blob, expectedLength, codec),
            "zstd" => DecodeZstd(blob, expectedLength),
            _ => throw new InvalidOperationException($"Unsupported codec: {codec}")
        };
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

    private static string ChunkRelativePath(string chunkSha256, string codec)
    {
        var suffix = codec switch
        {
            "zstd" => ".zst",
            "raw" => ".raw",
            _ => ".bin"
        };
        return Path.Combine(chunkSha256[..2], chunkSha256.Substring(2, 2), chunkSha256 + suffix);
    }

    private static string NormalizePrefix(string? prefix)
    {
        if (string.IsNullOrWhiteSpace(prefix))
            return string.Empty;

        return prefix.Trim().Trim('/').Trim('\\');
    }

    private static bool ShouldApplyManifestPrefix(string outRoot, string manifestPrefix)
    {
        if (string.IsNullOrWhiteSpace(manifestPrefix))
            return false;

        var normalizedOutRoot = outRoot
            .Replace('\\', '/')
            .TrimEnd('/');

        var normalizedPrefix = manifestPrefix
            .Replace('\\', '/')
            .Trim('/');

        var comparison = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
        if (string.Equals(normalizedOutRoot, normalizedPrefix, comparison))
            return false;

        return !normalizedOutRoot.EndsWith("/" + normalizedPrefix, comparison);
    }

    private static string ResolveEffectivePrefix(string outRoot, string manifestPrefix, IReadOnlyList<RigbyFile> files)
    {
        if (string.IsNullOrWhiteSpace(manifestPrefix))
            return string.Empty;

        if (ShouldApplyManifestPrefix(outRoot, manifestPrefix))
            return manifestPrefix;

        var sampleCount = Math.Min(files.Count, 256);
        var unprefixedExisting = 0;
        var prefixedExisting = 0;

        for (var i = 0; i < sampleCount; i++)
        {
            var file = files[i];
            var unprefixedPath = Path.Combine(outRoot, file.Path);
            if (File.Exists(unprefixedPath))
                unprefixedExisting += 1;

            var prefixedPath = Path.Combine(outRoot, manifestPrefix, file.Path);
            if (File.Exists(prefixedPath))
                prefixedExisting += 1;
        }

        return prefixedExisting > unprefixedExisting ? manifestPrefix : string.Empty;
    }

    private static void EnsureParent(string path)
    {
        var parent = Path.GetDirectoryName(path);
        if (!string.IsNullOrWhiteSpace(parent))
            Directory.CreateDirectory(parent);
    }

    private static IReadOnlyList<string> ExpandManifestInputs(IEnumerable<string> patterns)
    {
        var results = new List<string>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var cwd = Directory.GetCurrentDirectory();

        foreach (var raw in patterns)
        {
            if (string.IsNullOrWhiteSpace(raw))
                continue;

            if (IsHttpUrl(raw, out var manifestUri))
            {
                var url = manifestUri.AbsoluteUri;
                if (seen.Add(url))
                    results.Add(url);
                continue;
            }

            if (File.Exists(raw))
            {
                var fullPath = Path.GetFullPath(raw);
                if (seen.Add(fullPath))
                    results.Add(fullPath);
                continue;
            }

            var glob = raw.Replace('\\', '/');
            if (!glob.Contains('*') && !glob.Contains('?'))
                continue;

            foreach (var file in Directory.EnumerateFiles(cwd, "*", SearchOption.AllDirectories))
            {
                var relative = Path.GetRelativePath(cwd, file).Replace('\\', '/');
                if (!GlobMatch(relative, glob))
                    continue;

                var fullPath = Path.GetFullPath(file);
                if (seen.Add(fullPath))
                    results.Add(fullPath);
            }
        }

        return results;
    }

    private static bool IsHttpUrl(string value, out Uri uri)
    {
        if (Uri.TryCreate(value, UriKind.Absolute, out uri!)
            && (uri.Scheme == Uri.UriSchemeHttp || uri.Scheme == Uri.UriSchemeHttps))
            return true;

        uri = null!;
        return false;
    }

    private static bool GlobMatch(string input, string pattern)
    {
        var regex = "^" + Regex.Escape(pattern)
            .Replace("\\*\\*", "<<double-star>>")
            .Replace("\\*", "[^/]*")
            .Replace("<<double-star>>", ".*")
            .Replace("\\?", "[^/]")
            + "$";

        return Regex.IsMatch(input, regex, RegexOptions.CultureInvariant);
    }

    private readonly record struct RestoreTask(string OutputPath, string DisplayPath, RigbyFile File, IReadOnlyList<RigbyChunk> Chunks);
    private readonly record struct RestoreResult(bool Repaired, long DiskWriteBytes, long ReusedBytes);

    private static StringComparer GetPathComparer()
        => OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;

    private static bool IsPathWithinRoot(string rootPath, string candidatePath)
    {
        var relative = Path.GetRelativePath(rootPath, candidatePath);
        return relative != ".."
            && !relative.StartsWith($"..{Path.DirectorySeparatorChar}", StringComparison.Ordinal)
            && !relative.StartsWith($"..{Path.AltDirectorySeparatorChar}", StringComparison.Ordinal)
            && !Path.IsPathRooted(relative);
    }

    private static int DeleteUnexpectedFiles(string outRoot, HashSet<string> expectedFiles)
    {
        if (!Directory.Exists(outRoot))
            return 0;

        var deletedFiles = 0;
        foreach (var path in Directory.EnumerateFiles(outRoot, "*", SearchOption.AllDirectories))
        {
            var fullPath = Path.GetFullPath(path);
            if (expectedFiles.Contains(fullPath))
                continue;

            File.Delete(fullPath);
            deletedFiles += 1;
        }

        foreach (var dir in Directory
                     .EnumerateDirectories(outRoot, "*", SearchOption.AllDirectories)
                     .OrderByDescending(static x => x.Length))
        {
            if (Directory.EnumerateFileSystemEntries(dir).Any())
                continue;

            Directory.Delete(dir);
        }

        return deletedFiles;
    }

    private interface IRestoreProgress : IDisposable
    {
        void FileStarted(string path);
        void FileCompleted(RestoreResult result);
        void FileEnded(string path);
    }

    private sealed class JsonRestoreProgress : IRestoreProgress
    {
        private const int VisibleActiveLines = 3;
        private static readonly TimeSpan RenderInterval = TimeSpan.FromMilliseconds(500);

        private readonly object gate = new();
        private readonly Stopwatch stopwatch = Stopwatch.StartNew();
        private readonly int totalFiles;
        private readonly long totalBytes;
        private readonly HashSet<string> activeFiles = new(StringComparer.Ordinal);

        private TimeSpan lastRenderedAt = TimeSpan.MinValue;
        private int completedFiles;
        private long completedBytes;
        private int repairedFiles;
        private int verifiedFiles;
        private long diskWriteBytes;
        private long reusedBytes;

        public JsonRestoreProgress(int totalFiles, long totalBytes, string outDir)
        {
            this.totalFiles = totalFiles;
            this.totalBytes = totalBytes;
            Console.WriteLine(JsonSerializer.Serialize(
                new RigbyStartEvent("start", totalFiles, totalBytes, outDir),
                RigbyJsonOutputContext.Default.RigbyStartEvent));
        }

        public void FileStarted(string path)
        {
            lock (gate)
            {
                activeFiles.Add(path);
                Render();
            }
        }

        public void FileCompleted(RestoreResult result)
        {
            lock (gate)
            {
                completedFiles += 1;
                completedBytes += result.DiskWriteBytes + result.ReusedBytes;
                if (result.Repaired)
                    repairedFiles += 1;
                else
                    verifiedFiles += 1;

                diskWriteBytes += result.DiskWriteBytes;
                reusedBytes += result.ReusedBytes;
                Render(force: true);
            }
        }

        public void FileEnded(string path)
        {
            lock (gate)
            {
                activeFiles.Remove(path);
                Render();
            }
        }

        public void Dispose()
        {
            // No-op. Final completion event is emitted by Restore() only on success.
        }

        private void Render(bool force = false)
        {
            var now = stopwatch.Elapsed;
            if (!force && lastRenderedAt != TimeSpan.MinValue && now - lastRenderedAt < RenderInterval)
                return;

            lastRenderedAt = now;

            var elapsed = Math.Max(0.001, stopwatch.Elapsed.TotalSeconds);
            var percent = totalFiles == 0 ? 100.0 : completedFiles * 100.0 / totalFiles;
            var speedMiB = (completedBytes / 1024.0 / 1024.0) / elapsed;
            var remainingBytes = Math.Max(0L, totalBytes - completedBytes);
            var etaSeconds = speedMiB > 0.01
                ? (int)Math.Round(remainingBytes / (speedMiB * 1024 * 1024))
                : (int?)null;

            Console.WriteLine(JsonSerializer.Serialize(
                new RigbyProgressEvent(
                    "progress",
                    percent,
                    completedFiles,
                    totalFiles,
                    completedBytes,
                    totalBytes,
                    speedMiB,
                    etaSeconds,
                    repairedFiles,
                    verifiedFiles,
                    diskWriteBytes,
                    reusedBytes,
                    activeFiles.Take(VisibleActiveLines).ToArray()),
                RigbyJsonOutputContext.Default.RigbyProgressEvent));
        }
    }

    private sealed class RestoreProgress : IRestoreProgress
    {
        private const int BarWidth = 20;
        private const int VisibleActiveLines = 3;
        private const int MaxFileLineLength = 88;
        private static readonly TimeSpan RenderInterval = TimeSpan.FromMilliseconds(500);
        private static readonly char[] SpinnerFrames = ['|', '/', '-', '\\'];

        private readonly object gate = new();
        private readonly Stopwatch stopwatch = Stopwatch.StartNew();
        private readonly int totalFiles;
        private readonly long totalBytes;
        private readonly HashSet<string> activeFiles = new(StringComparer.Ordinal);

        private bool hasRendered;
        private int renderedLines;
        private TimeSpan lastRenderedAt = TimeSpan.MinValue;
        private int completedFiles;
        private long completedBytes;
        private int repairedFiles;
        private int verifiedFiles;
        private long diskWriteBytes;
        private long reusedBytes;

        public RestoreProgress(int totalFiles, long totalBytes)
        {
            this.totalFiles = totalFiles;
            this.totalBytes = totalBytes;
            Render(force: true);
        }

        public void FileStarted(string path)
        {
            lock (gate)
            {
                activeFiles.Add(path);
                Render();
            }
        }

        public void FileCompleted(RestoreResult result)
        {
            lock (gate)
            {
                completedFiles += 1;
                completedBytes += result.DiskWriteBytes + result.ReusedBytes;
                if (result.Repaired)
                    repairedFiles += 1;
                else
                    verifiedFiles += 1;

                diskWriteBytes += result.DiskWriteBytes;
                reusedBytes += result.ReusedBytes;
                Render();
            }
        }

        public void FileEnded(string path)
        {
            lock (gate)
            {
                activeFiles.Remove(path);
                Render();
            }
        }

        public void Dispose()
        {
            lock (gate)
            {
                Render(force: true);
                ClearRenderedBlock();
            }
        }

        private void Render(bool force = false)
        {
            var now = stopwatch.Elapsed;
            if (!force && lastRenderedAt != TimeSpan.MinValue && now - lastRenderedAt < RenderInterval)
                return;

            lastRenderedAt = now;

            var elapsed = Math.Max(0.001, stopwatch.Elapsed.TotalSeconds);
            var percent = totalFiles == 0 ? 100.0 : completedFiles * 100.0 / totalFiles;
            var filled = (int)Math.Round(percent / 100.0 * BarWidth);
            var bar = new string('=', Math.Clamp(filled, 0, BarWidth)) + new string(' ', Math.Max(0, BarWidth - filled));
            var spinner = SpinnerFrames[(int)(now.TotalMilliseconds / 180) % SpinnerFrames.Length];

            var completedMiB = completedBytes / 1024.0 / 1024.0;
            var totalMiB = totalBytes / 1024.0 / 1024.0;
            var speedMiB = completedMiB / elapsed;
            var remainingMiB = Math.Max(0.0, totalMiB - completedMiB);
            var eta = speedMiB > 0.01 ? TimeSpan.FromSeconds(remainingMiB / speedMiB) : TimeSpan.Zero;

            var active = activeFiles.Take(VisibleActiveLines).ToArray();
            var lines = new List<string>
            {
                $"{spinner} Rigby restore  {percent,5:0.0}% [{bar}]",
                $"  Throughput {speedMiB,6:0.0} MiB/s   Data {completedMiB:0}/{totalMiB:0} MiB",
                $"  Files      {completedFiles}/{totalFiles}   Repaired {repairedFiles}   Verified {verifiedFiles}",
                $"  Storage    Write {FormatMiB(diskWriteBytes)}   Reused {FormatMiB(reusedBytes)}   ETA {(eta == TimeSpan.Zero ? "--:--" : $"{eta:mm\\:ss}")}",
                "  In flight:"
            };

            for (var i = 0; i < VisibleActiveLines; i++)
                lines.Add(i < active.Length ? $"  - {Shorten(active[i], MaxFileLineLength)}" : "  -");

            if (hasRendered && renderedLines > 0)
                Console.Error.Write($"\u001b[{renderedLines}F");

            for (var i = 0; i < lines.Count; i++)
            {
                Console.Error.Write("\u001b[2K");
                Console.Error.Write(lines[i]);
                Console.Error.WriteLine();
            }

            hasRendered = true;
            renderedLines = lines.Count;
        }

        private static string Shorten(string input, int maxLen)
        {
            if (input.Length <= maxLen)
                return input;

            const string marker = "...";
            var keep = Math.Max(0, maxLen - marker.Length);
            return marker + input[^keep..];
        }

        private void ClearRenderedBlock()
        {
            if (!hasRendered || renderedLines == 0)
                return;

            Console.Error.Write($"\u001b[{renderedLines}F");
            for (var i = 0; i < renderedLines; i++)
            {
                Console.Error.Write("\u001b[2K");
                if (i < renderedLines - 1)
                    Console.Error.Write("\u001b[1E");
            }

            if (renderedLines > 1)
                Console.Error.Write($"\u001b[{renderedLines - 1}F");

            hasRendered = false;
            renderedLines = 0;
        }
    }
}

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

internal sealed class RigbyFile
{
    public required string Path { get; init; }
    public required long Size { get; init; }
    public required string Sha256 { get; init; }
    public required string Md5 { get; init; }
    public string? Blake3 { get; init; }
    public required int ChunkStart { get; init; }
    public required int ChunkEnd { get; init; }
}

internal sealed class RigbyChunk
{
    public required string Sha256 { get; init; }
    public required string Md5 { get; init; }
    public string? Blake3 { get; init; }
    public required int Length { get; init; }
    public string? Codec { get; init; }
}

internal sealed record RigbyStartEvent(string Type, int FilesTotal, long BytesTotal, string OutDir);
internal sealed record RigbyProgressEvent(
    string Type,
    double Percent,
    int FilesCompleted,
    int FilesTotal,
    long BytesCompleted,
    long BytesTotal,
    double SpeedMiB,
    int? EtaSeconds,
    int RepairedFiles,
    int VerifiedFiles,
    long DiskWriteBytes,
    long ReusedBytes,
    string[] ActiveFiles);
internal sealed record RigbyCompleteEvent(
    string Type,
    int Files,
    string OutDir,
    int RepairedFiles,
    int VerifiedFiles,
    int DeletedFiles,
    long DiskWriteBytes,
    long ReusedBytes);

[System.Text.Json.Serialization.JsonSourceGenerationOptions(PropertyNamingPolicy = System.Text.Json.Serialization.JsonKnownNamingPolicy.CamelCase)]
[System.Text.Json.Serialization.JsonSerializable(typeof(RigbyStartEvent))]
[System.Text.Json.Serialization.JsonSerializable(typeof(RigbyProgressEvent))]
[System.Text.Json.Serialization.JsonSerializable(typeof(RigbyCompleteEvent))]
internal partial class RigbyJsonOutputContext : System.Text.Json.Serialization.JsonSerializerContext
{
}
