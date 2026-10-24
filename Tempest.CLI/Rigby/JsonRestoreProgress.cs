using System.Diagnostics;
using System.Text.Json;
using Tempest.CLI.Rigby.Events;
using Tempest.CLI.Rigby.Models;

namespace Tempest.CLI.Rigby;

internal sealed class JsonRestoreProgress(int totalFiles, long totalBytes) : IRestoreProgress
{
    private static readonly TimeSpan EmitInterval = TimeSpan.FromMilliseconds(100);
    private static readonly TimeSpan SpeedWindow = TimeSpan.FromSeconds(2);

    private readonly Lock gate = new();
    private readonly Stopwatch stopwatch = Stopwatch.StartNew();
    private readonly int totalFiles = totalFiles;
    private readonly long totalBytes = totalBytes;
    private readonly Queue<(TimeSpan At, long Work)> recentWork = new();

    private int completedFiles;
    private long completedBytes;
    private int repairedFiles;
    private int verifiedFiles;
    private long diskWriteBytes;
    private long reusedBytes;
    private long resumedBytes;
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

    // The resumed counts don't emit: Flush reports their total at once, so the first event of a
    // resumed run already shows where the interrupted one stopped.
    public void FileResumed(long size, RestoreResult result)
    {
        lock (gate)
        {
            completedFiles += 1;
            if (result.Repaired)
                repairedFiles += 1;
            else
                verifiedFiles += 1;

            diskWriteBytes += result.DiskWriteBytes;
            reusedBytes += result.ReusedBytes;
            completedBytes += size;
            resumedBytes += size;
        }
    }

    public void BytesResumed(long count)
    {
        lock (gate)
        {
            reusedBytes += count;
            completedBytes += count;
            resumedBytes += count;
        }
    }

    public void Flush()
    {
        lock (gate)
            Emit(force: true);
    }

    private void Emit(bool force)
    {
        var now = stopwatch.Elapsed;
        if (!force && lastEmittedAt != TimeSpan.MinValue && now - lastEmittedAt < EmitInterval)
            return;

        lastEmittedAt = now;

        var work = completedBytes - resumedBytes;
        var averageSpeed = work / Math.Max(0.001, now.TotalSeconds);
        var remainingBytes = Math.Max(0, totalBytes - completedBytes);
        var etaSeconds = averageSpeed > 10 * 1024 ? remainingBytes / averageSpeed : 0;

        var progressEvent = new RigbyProgressEvent(
            "progress",
            completedFiles,
            totalFiles,
            totalBytes == 0 ? 100.0 : completedBytes * 100.0 / totalBytes,
            completedBytes,
            totalBytes,
            CurrentSpeed(now, work, averageSpeed),
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

    /// <summary>
    /// Speed over the last few seconds, so the launcher's graph shows the download as it is now;
    /// the time left stays on the run's average, which doesn't jump with every dip.
    /// </summary>
    private double CurrentSpeed(TimeSpan now, long work, double averageSpeed)
    {
        recentWork.Enqueue((now, work));
        while (recentWork.Count > 1 && now - recentWork.Peek().At > SpeedWindow)
            recentWork.Dequeue();

        var (since, workThen) = recentWork.Peek();
        var seconds = (now - since).TotalSeconds;
        return seconds >= 1 ? (work - workThen) / seconds : averageSpeed;
    }

    public void Dispose()
    {
    }
}
