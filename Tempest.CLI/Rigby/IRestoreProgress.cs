using Tempest.CLI.Rigby.Models;

namespace Tempest.CLI.Rigby;

internal interface IRestoreProgress : IDisposable
{
    void BytesWritten(long count);

    void BytesReused(long count);

    void FileCompleted(RestoreResult result);

    /// <summary>A file an earlier, interrupted run finished: done, but not work this run did.</summary>
    void FileResumed(long size, RestoreResult result);

    /// <summary>The start of a file an earlier, interrupted run wrote, kept as it is.</summary>
    void BytesResumed(long count);

    /// <summary>Reports the totals now instead of with the next change.</summary>
    void Flush();
}
