using ConsoleAppFramework;

namespace Tempest.CLI.Server;

internal class ServerCommands
{
    public async Task Open(
        [Argument] string path,
        string name = "Paladins Server",
        string tags = "",
        string map = "",
        string version = "0.57",
        uint maxPlayers = 10,
        bool joinInProgress = false,
        bool publicServer = false,
        string? gamemode = null,
        string? publicIp = null,
        string? privateId = null,
        string servicesUrl = "https://localhost:7165")
    {
    }
}