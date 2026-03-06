using Tempest.CLI.Rigby.Models;

namespace Tempest.CLI.Rigby;

internal interface IRestoreProgress : IDisposable
{
    void FileCompleted(RestoreResult result);
}
