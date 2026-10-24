using System.Globalization;
using Tempest.CLI.Rigby.Models;

namespace Tempest.CLI.Rigby;

/// <summary>
/// The files a restore has finished, appended as it goes so a restore that was killed (paused)
/// can skip them with <c>--resume</c> instead of hashing everything again. An entry only counts
/// while the file still has the size and write time the restore left it with.
/// </summary>
internal sealed class RigbyResumeState : IDisposable
{
    private const string FileName = ".rigby-resume";

    private readonly Lock gate = new();
    private readonly string outRoot;
    private readonly Dictionary<string, Entry> entries;
    private readonly StreamWriter writer;

    private RigbyResumeState(string outRoot, Dictionary<string, Entry> entries, StreamWriter writer)
    {
        this.outRoot = outRoot;
        this.entries = entries;
        this.writer = writer;
    }

    public static RigbyResumeState Open(string outRoot, bool resume)
    {
        var path = Path.Combine(outRoot, FileName);
        var entries = resume
            ? Load(outRoot, path)
            : new Dictionary<string, Entry>(RigbyOutputLayout.GetPathComparer());

        var stream = new FileStream(path, resume ? FileMode.Append : FileMode.Create, FileAccess.Write, FileShare.Read);
        var writer = new StreamWriter(stream) { AutoFlush = true, NewLine = "\n" };
        // Ends a line the kill may have cut short, so the first new entry isn't glued onto it.
        if (resume)
            writer.WriteLine();

        return new RigbyResumeState(outRoot, entries, writer);
    }

    public static void Delete(string outRoot) => File.Delete(Path.Combine(outRoot, FileName));

    public bool TryGetFinished(RestoreTask task, out RestoreResult result)
    {
        result = default;
        if (!entries.TryGetValue(task.OutputPath, out var entry)
            || entry.Size != task.File.Size
            || !string.Equals(entry.Sha256, task.File.Sha256, StringComparison.OrdinalIgnoreCase))
            return false;

        var info = new FileInfo(task.OutputPath);
        if (!info.Exists || info.Length != entry.Size || info.LastWriteTimeUtc.Ticks != entry.WriteTicks)
            return false;

        result = entry.Result;
        return true;
    }

    public void MarkFinished(RestoreTask task, RestoreResult result)
    {
        var writeTicks = new FileInfo(task.OutputPath).LastWriteTimeUtc.Ticks;
        var relative = Path.GetRelativePath(outRoot, task.OutputPath);
        var line = string.Create(
            CultureInfo.InvariantCulture,
            $"{task.File.Sha256}\t{task.File.Size}\t{writeTicks}\t{(result.Repaired ? 1 : 0)}\t{result.DiskWriteBytes}\t{result.ReusedBytes}\t{relative}");

        lock (gate)
            writer.WriteLine(line);
    }

    public void Dispose() => writer.Dispose();

    private static Dictionary<string, Entry> Load(string outRoot, string path)
    {
        var entries = new Dictionary<string, Entry>(RigbyOutputLayout.GetPathComparer());
        if (!File.Exists(path))
            return entries;

        foreach (var line in File.ReadLines(path))
        {
            var parts = line.Split('\t', 7);
            if (parts.Length != 7
                || !TryParse(parts[1], out var size)
                || !TryParse(parts[2], out var writeTicks)
                || parts[3] is not ("0" or "1")
                || !TryParse(parts[4], out var diskWriteBytes)
                || !TryParse(parts[5], out var reusedBytes))
                continue;

            var fullPath = Path.GetFullPath(Path.Combine(outRoot, parts[6]));
            entries[fullPath] = new Entry(
                parts[0],
                size,
                writeTicks,
                new RestoreResult(parts[3] == "1", diskWriteBytes, reusedBytes));
        }

        return entries;
    }

    private static bool TryParse(string text, out long value)
        => long.TryParse(text, NumberStyles.None, CultureInfo.InvariantCulture, out value);

    private readonly record struct Entry(string Sha256, long Size, long WriteTicks, RestoreResult Result);
}
