using Tempest.CLI.Rigby.Models;

namespace Tempest.CLI.Rigby;

internal interface IRestoreProgress : IDisposable
{
    void BytesWritten(long count);

    void BytesReused(long count);

    void FileCompleted(RestoreResult result);
}
