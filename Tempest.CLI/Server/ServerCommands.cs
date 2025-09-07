using ConsoleAppFramework;
using System.Net;
using System.Runtime.CompilerServices;

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
        string servicesUrl = "https://localhost:7165",
        int port = 50051,
        string? password = null,
        bool detach = false)
    {
        // Create and start embedded gRPC + HTTP status server
        var options = new LobbyServerOptions
        {
            Name = name,
            MaxPlayers = (int)maxPlayers,
            Password = password,
            Map = map,
            Version = version,
            Tags = tags.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries),
            JoinInProgress = joinInProgress,
            PublicServer = publicServer,
            GameMode = gamemode,
            Port = port
        };

        var server = new EmbeddedServer(options);
        await server.StartAsync();

        Console.WriteLine($"Lobby '{options.Name}' started on port {port} (gRPC + HTTP)");
        Console.WriteLine("Players can now connect (stub logic). Press Ctrl+C to stop.");

        if (detach)
        {
            // Detach immediately; background server keeps running until process exit.
            return;
        }

        var tcs = new TaskCompletionSource();
        Console.CancelKeyPress += (_, e) =>
        {
            e.Cancel = true;
            if (!tcs.Task.IsCompleted)
                tcs.SetResult();
        };

        await tcs.Task; // Wait until cancellation (Ctrl+C)
        await server.StopAsync();
    }
}