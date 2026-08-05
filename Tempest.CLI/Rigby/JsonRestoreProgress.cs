using System.Diagnostics;
using System.Text.Json;
using Tempest.CLI.Rigby.Events;
using Tempest.CLI.Rigby.Models;

namespace Tempest.CLI.Rigby;

internal sealed class JsonRestoreProgress(int totalFiles, long totalBytes) : IRestoreProgress
{
    private static readonly TimeSpan EmitInterval = TimeSpan.FromMilliseconds(100);

    private readonly Lock gate = new();
    private readonly Stopwatch stopwatch = Stopwatch.StartNew();
    private readonly int totalFiles = totalFiles;
    private readonly long totalBytes = totalBytes;

    private int completedFiles;
    private long completedBytes;
    private int repairedFiles;
    private int verifiedFiles;
    private long diskWriteBytes;
    private long reusedBytes;
    private TimeSpan lastEmittedAt = TimeSpan.MinValue;

    public void BytesWritten(long count)
    {
        ReportBytes(count, 0);
    }

    public void BytesReused(long count)
    {
        ReportBytes(0, count);
    }

    private void ReportBytes(long written, long reused)
    {
        lock (gate)
        {
            diskWriteBytes += written;
            reusedBytes += reused;
            completedBytes += written + reused;
            Emit(force: false);
        }
    }

    public void FileCompleted(RestoreResult result)
    {
        lock (gate)
        {
            completedFiles += 1;
            if (result.Repaired)
                repairedFiles += 1;
            else
                verifiedFiles += 1;

            Emit(force: false);
        }
    }

    private void Emit(bool force)
    {
        var now = stopwatch.Elapsed;
        if (!force && lastEmittedAt != TimeSpan.MinValue && now - lastEmittedAt < EmitInterval)
            return;

        lastEmittedAt = now;

        var elapsed = Math.Max(0.001, stopwatch.Elapsed.TotalSeconds);
        var percent = totalBytes == 0 ? 100.0 : completedBytes * 100.0 / totalBytes;
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

    public void Dispose()
    {
    }
}
