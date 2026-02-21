namespace Tempest.CLI.Rigby.Models;

internal sealed class RigbyFile
{
    public required string Path { get; init; }
    public required long Size { get; init; }
    public required string Sha256 { get; init; }
    public required string Md5 { get; init; }
    public string? Blake3 { get; init; }
    public required int ChunkStart { get; init; }
    public required int ChunkEnd { get; init; }
}
