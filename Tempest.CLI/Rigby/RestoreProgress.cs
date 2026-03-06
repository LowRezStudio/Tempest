using System.Diagnostics;
using System.Threading;
using Tempest.CLI.Rigby.Models;

namespace Tempest.CLI.Rigby;

internal sealed class RestoreProgress : IRestoreProgress
{
    private const int BarWidth = 20;
    private static readonly TimeSpan RenderInterval = TimeSpan.FromMilliseconds(500);
    private static readonly char[] SpinnerFrames = ['|', '/', '-', '\\'];

    private readonly Lock gate = new();
    private readonly Stopwatch stopwatch = Stopwatch.StartNew();
    private readonly int totalFiles;
    private readonly long totalBytes;

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

        var lines = new List<string>
        {
            $"{spinner} Rigby restore  {percent,5:0.0}% [{bar}]",
            $"  Throughput {speedMiB,6:0.0} MiB/s   Data {completedMiB:0}/{totalMiB:0} MiB",
            $"  Files      {completedFiles}/{totalFiles}   Repaired {repairedFiles}   Verified {verifiedFiles}",
            $"  Storage    Write {diskWriteBytes / 1024.0 / 1024.0:0.0}MiB   Reused {reusedBytes / 1024.0 / 1024.0:0.0}MiB   ETA {(eta == TimeSpan.Zero ? "--:--" : $"{eta:mm\\:ss}")}"
        };

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
