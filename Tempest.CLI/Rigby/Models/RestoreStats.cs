namespace Tempest.CLI.Rigby.Models;

internal sealed class RestoreStats(int totalFiles)
{
    private int repairedFiles;
    private int verifiedFiles;
    private long diskWriteBytes;
    private long reusedBytes;

    public int TotalFiles { get; } = totalFiles;
    public int RepairedFiles => repairedFiles;
    public int VerifiedFiles => verifiedFiles;
    public long DiskWriteBytes => diskWriteBytes;
    public long ReusedBytes => reusedBytes;

    public void Add(RestoreResult result)
    {
        if (result.Repaired)
            Interlocked.Increment(ref repairedFiles);
        else
            Interlocked.Increment(ref verifiedFiles);

        Interlocked.Add(ref diskWriteBytes, result.DiskWriteBytes);
        Interlocked.Add(ref reusedBytes, result.ReusedBytes);
    }
}
