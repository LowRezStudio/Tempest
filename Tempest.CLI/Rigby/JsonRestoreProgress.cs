using System.Diagnostics;
using System.Text.Json;
using Tempest.CLI.Rigby.Events;
using Tempest.CLI.Rigby.Models;

namespace Tempest.CLI.Rigby;

internal sealed class JsonRestoreProgress : IRestoreProgress
{
    private readonly Lock gate = new();
    private readonly Stopwatch stopwatch = Stopwatch.StartNew();
    private readonly int totalFiles;
    private readonly long totalBytes;

    private int completedFiles;
    private long completedBytes;
    private int repairedFiles;
    private int verifiedFiles;
    private long diskWriteBytes;
    private long reusedBytes;

    public JsonRestoreProgress(int totalFiles, long totalBytes)
    {
        this.totalFiles = totalFiles;
        this.totalBytes = totalBytes;
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

            var elapsed = Math.Max(0.001, stopwatch.Elapsed.TotalSeconds);
            var percent = totalFiles == 0 ? 100.0 : completedFiles * 100.0 / totalFiles;
            var completedMiB = completedBytes / 1024.0 / 1024.0;
            var totalMiB = totalBytes / 1024.0 / 1024.0;
            var speedMiB = completedMiB / elapsed;
            var remainingMiB = Math.Max(0.0, totalMiB - completedMiB);
            var etaSeconds = speedMiB > 0.01 ? remainingMiB / speedMiB : 0;

            var progressEvent = new RigbyProgressEvent(
                "progress",
                completedFiles,
                totalFiles,
                percent,
                completedBytes,
                totalBytes,
                speedMiB * 1024 * 1024,
                etaSeconds,
                repairedFiles,
                verifiedFiles,
                diskWriteBytes,
                reusedBytes
            );

            Console.WriteLine(JsonSerializer.Serialize(
                progressEvent,
                RigbyJsonOutputContext.Default.RigbyProgressEvent));
        }
    }

    public void Dispose()
    {
    }
}