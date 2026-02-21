namespace Tempest.CLI.Rigby.Models;

internal sealed class RigbyChunk
{
    public required string Sha256 { get; init; }
    public required string Md5 { get; init; }
    public string? Blake3 { get; init; }
    public required int Length { get; init; }
    public string? Codec { get; init; }
}
