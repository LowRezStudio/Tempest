namespace Tempest.CLI.Rigby.Events;

public sealed record RigbyCompleteEvent(
    string Type,
    int Files,
    string OutDir,
    int RepairedFiles,
    int VerifiedFiles,
    int DeletedFiles,
    long DiskWriteBytes,
    long ReusedBytes);
