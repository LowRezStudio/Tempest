using System.Text.Json;
using ConsoleAppFramework;
using Tempest.CLI.Rigby.Events;
using Tempest.CLI.Rigby.Models;

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

        var manifestPaths = RigbyManifestInputResolver.ExpandManifestInputs(manifests);
        if (manifestPaths.Count == 0)
            throw new InvalidOperationException("No manifest files matched the provided inputs.");

        var outRoot = Path.GetFullPath(outDir);
        Directory.CreateDirectory(outRoot);

        var chunksRootFull = string.IsNullOrWhiteSpace(chunksRoot) ? null : Path.GetFullPath(chunksRoot);
        if (chunksRootFull is not null && !Directory.Exists(chunksRootFull))
            throw new InvalidOperationException($"Chunks root does not exist: {chunksRootFull}");

        using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(30.0) };

        var tasks = new List<RestoreTask>();
        var expectedFiles = new HashSet<string>(RigbyOutputLayout.GetPathComparer());

        foreach (var manifestPath in manifestPaths)
        {
            var manifest = await RigbyManifest.LoadAsync(manifestPath, http, cancellationToken);
            if (!string.Equals(manifest.FormatVersion, FormatVersion, StringComparison.Ordinal))
                throw new InvalidOperationException($"Unsupported manifest format in {manifestPath}: {manifest.FormatVersion}");

            var manifestPrefix = RigbyOutputLayout.NormalizePrefix(manifest.PathPrefix);
            var prefix = RigbyOutputLayout.ResolveEffectivePrefix(outRoot, manifestPrefix, manifest.Files);

            foreach (var file in manifest.Files)
            {
                var relPath = string.IsNullOrEmpty(prefix) ? file.Path : Path.Combine(prefix, file.Path);
                var outputPath = Path.GetFullPath(Path.Combine(outRoot, relPath));
                if (!RigbyOutputLayout.IsPathWithinRoot(outRoot, outputPath))
                    throw new InvalidOperationException($"Manifest path escapes output directory: {relPath}");

                tasks.Add(new RestoreTask(outputPath, file, manifest.Chunks));
                expectedFiles.Add(outputPath);
            }
        }

        var stats = await RunRestoreAsync(tasks, chunksRootFull, baseUrl, noDownload, http, cancellationToken);
        var deletedFiles = RigbyOutputLayout.DeleteUnexpectedFiles(outRoot, expectedFiles);

        if (json)
        {
            Console.WriteLine(JsonSerializer.Serialize(
                new RigbyCompleteEvent(
                    "complete",
                    stats.TotalFiles,
                    outRoot,
                    stats.RepairedFiles,
                    stats.VerifiedFiles,
                    deletedFiles,
                    stats.DiskWriteBytes,
                    stats.ReusedBytes),
                RigbyJsonOutputContext.Default.RigbyCompleteEvent));
        }
        else
        {
            Console.WriteLine(
                $"Restore complete. files={stats.TotalFiles}, repaired={stats.RepairedFiles}, verified={stats.VerifiedFiles}, deleted={deletedFiles}, " +
                $"disk_write_bytes={stats.DiskWriteBytes}, reused_bytes={stats.ReusedBytes}, out={outRoot}");
        }
    }

    private static async Task<RestoreStats> RunRestoreAsync(
        IReadOnlyList<RestoreTask> tasks,
        string? chunksRoot,
        string? baseUrl,
        bool noDownload,
        HttpClient http,
        CancellationToken cancellationToken)
    {
        var totalFiles = tasks.Count;
        var totalBytes = tasks.Sum(static x => x.File.Size);
        var stats = new RestoreStats(totalFiles);
        if (totalFiles == 0)
            return stats;

        using IRestoreProgress progress = new RestoreProgress(totalFiles, totalBytes);

        await Parallel.ForEachAsync(
            tasks,
            new ParallelOptions
            {
                MaxDegreeOfParallelism = Math.Max(1, Environment.ProcessorCount),
                CancellationToken = cancellationToken
            },
            async (task, ct) =>
            {
                var result = await RigbyRestoreEngine.RestoreOneAsync(task, chunksRoot, baseUrl, noDownload, http, ct);
                stats.Add(result);
                progress.FileCompleted(result);
            });

        return stats;
    }
}
