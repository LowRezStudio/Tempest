namespace Tempest.CLI.Rigby.Models;

internal readonly record struct RestoreTask(string OutputPath, RigbyFile File, IReadOnlyList<RigbyChunk> Chunks);
