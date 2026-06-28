using System.Diagnostics.CodeAnalysis;
using Microsoft.Extensions.Logging;
using ZLogger;

namespace Tempest.CLI.Server;

internal class ServerCommands
{
    public async Task Open(
        string path,
        string name = "Paladins Server",
        string tags = "",
        string map = "",
        string version = "0.57",
        uint maxPlayers = 10,
        uint minPlayers = 4,
        bool joinInProgress = false,
        bool publicServer = false,
        string? gamemode = null,
        string servicesUrl = "https://api.lowrezstudio.com",
        int port = 50051,
        int gameServerPort = 7000,
        string? password = null,
        bool detach = false,
        bool noDefaultArgs = false,
        string? platform = null,
        string? game = null,
        string[]? dll = null,
        bool enableJoinInProgress = false,
        bool upnp = false,
        CancellationToken cancellationToken = default)
    {
        var options = new LobbyServerOptions
        {
            Name = name,
            MaxPlayers = (int)maxPlayers,
            MinPlayers = (int)minPlayers,
            Password = password,
            Map = map,
            Version = version,
            Tags = tags.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries),
            JoinInProgress = joinInProgress,
            PublicServer = publicServer,
            GameMode = gamemode,
            Port = port,
            GameServerPort = gameServerPort,
            ServicesUrl = servicesUrl,
            Path = path,
            NoDefaultArgs = noDefaultArgs,
            Platform = platform,
            Game = game,
            Dll = dll,
            EnableJoinInProgress = enableJoinInProgress,
            Upnp = upnp,
        };

        using var loggerFactory = LoggerFactory.Create(logging =>
        {
            logging.ClearProviders();
            logging.AddZLoggerConsole();
        });

        var server = new EmbeddedServer(options, loggerFactory);
        await server.StartAsync();

        Console.WriteLine($"Lobby '{options.Name}' started on port {port} (gRPC + HTTP)");
        Console.WriteLine("Players can now connect (stub logic). Press Ctrl+C to stop.");

        if (detach)
        {
            return;
        }

        using var cts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);

        _ = Task.Run(async () =>
        {
            try
            {
                using var reader = new StreamReader(Console.OpenStandardInput());
                while (!cts.Token.IsCancellationRequested)
                {
                    string? input;
                    try
                    {
                        input = await reader.ReadLineAsync(cts.Token);
                    }
                    catch (OperationCanceledException)
                    {
                        break;
                    }

                    if (input == null)
                    {
                        try { await Task.Delay(100, cts.Token); }
                        catch (OperationCanceledException) { break; }
                        continue;
                    }

                    if (input.Trim().Equals("kill", StringComparison.OrdinalIgnoreCase))
                    {
                        await cts.CancelAsync();
                        break;
                    }
                }
            }
            catch (OperationCanceledException)
            {
                // Expected when shutdown is requested.
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine($"Stdin reader error: {ex.Message}");
            }
        }, cts.Token);

        try
        {
            await Task.Delay(Timeout.Infinite, cts.Token);
        }
        catch (OperationCanceledException)
        {
            Console.WriteLine("Shutting down...");
        }

        await server.StopAsync();
    }

    public async Task List(string servicesUrl = "https://api.lowrezstudio.com")
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

    public async Task Get(string id, string servicesUrl = "https://api.lowrezstudio.com")
    {
        using var client = new ServerListClient(servicesUrl);

        try
        {
            var response = await client.GetServerByIdAsync(id);

            if (response.ResultCase == Protocol.ServerList.GetServerByIdResponse.ResultOneofCase.Success)
            {
                var server = response.Success;
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
            else
            {
                Console.WriteLine($"Response Error: {response.Error.Message}");
            }
        }
        catch (Exception ex)
        {
            Console.WriteLine($"Error: {ex.Message}");
        }
    }
}
