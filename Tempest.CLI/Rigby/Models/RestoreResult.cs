namespace Tempest.CLI.Rigby.Models;

internal readonly record struct RestoreResult(bool Repaired, long DiskWriteBytes, long ReusedBytes);
