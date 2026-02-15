using ConsoleAppFramework;
using System.Net;
using System.Runtime.CompilerServices;

namespace Tempest.CLI.Server;

internal class ServerCommands
{
    public async Task Open(
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
            Port = port,
            ServicesUrl = servicesUrl
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

        await tcs.Task;
        await server.StopAsync();
    }

    public async Task List(string servicesUrl = "https://localhost:7165")
    {
        using var client = new ServerListClient(servicesUrl);
        var servers = await client.GetServersAsync();

        if (servers.Count == 0)
        {
            Console.WriteLine("No servers found.");
            return;
        }

        Console.WriteLine($"Found {servers.Count} server(s):\n");

        foreach (var server in servers)
        {
            Console.WriteLine($"ID: {server.Id}");
            Console.WriteLine($"Name: {server.Name}");
            Console.WriteLine($"IP: {server.Ip}:{server.LobbyPort}");
            Console.WriteLine($"Game: {server.Game} v{server.Version}");
            Console.WriteLine($"Players: {server.Players}/{server.MaxPlayers}");
            Console.WriteLine($"Map: {server.Map ?? "N/A"}");
            Console.WriteLine($"Joinable: {server.Joinable}");
            Console.WriteLine($"Password: {(server.HasPassword ? "Yes" : "No")}");
            if (server.Tags.Count > 0)
                Console.WriteLine($"Tags: {string.Join(", ", server.Tags)}");
            Console.WriteLine();
        }
    }

    public async Task Get(string id, string servicesUrl = "https://localhost:7165")
    {
        using var client = new ServerListClient(servicesUrl);
        
        try
        {
            var server = await client.GetServerByIdAsync(id);
            
            Console.WriteLine($"ID: {server.Id}");
            Console.WriteLine($"Name: {server.Name}");
            Console.WriteLine($"IP: {server.Ip}:{server.LobbyPort}");
            Console.WriteLine($"Game: {server.Game} v{server.Version}");
            Console.WriteLine($"Players: {server.Players}/{server.MaxPlayers}");
            Console.WriteLine($"Bots: {server.Bots}");
            Console.WriteLine($"Spectators: {server.Spectators}/{server.MaxSpectators}");
            Console.WriteLine($"Map: {server.Map ?? "N/A"}");
            Console.WriteLine($"Map ID: {server.MapId ?? "N/A"}");
            Console.WriteLine($"Joinable: {server.Joinable}");
            Console.WriteLine($"Join in Progress: {server.JoinInProgress}");
            Console.WriteLine($"Password: {(server.HasPassword ? "Yes" : "No")}");
            Console.WriteLine($"Country: {server.Country}");
            if (server.Tags.Count > 0)
                Console.WriteLine($"Tags: {string.Join(", ", server.Tags)}");
        }
        catch (Exception ex)
        {
            Console.WriteLine($"Error: {ex.Message}");
        }
    }
}