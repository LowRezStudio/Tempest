namespace Tempest.CLI.Server;

internal sealed class LobbyServerOptions
{
    public required string Name { get; init; }
    public required int MaxPlayers { get; init; }
    public required int MinPlayers { get; init; }
    public string? Password { get; init; }
    public string Map { get; init; } = string.Empty;
    public string Version { get; init; } = string.Empty;
    public IEnumerable<string> Tags { get; init; } = Enumerable.Empty<string>();
    public bool JoinInProgress { get; init; }
    public bool PublicServer { get; init; }
    public string? GameMode { get; init; }
    public int Port { get; init; } = 50051;
    public string? ServicesUrl { get; init; }
    public string Path { get; init; } = string.Empty;
    public bool NoDefaultArgs { get; init; } = false;
    public string? Platform { get; init; } = null;
    public string? Game { get; init; } = null;
    public string[]? Dll { get; init; } = null;
}
