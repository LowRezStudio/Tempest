using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Net.NetworkInformation;
using ZLogger;
using Tempest.Protocol.ServerList;

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
        bool discover = false,
        string? apiKey = null,
        string country = "UNSPECIFIED",
        CancellationToken cancellationToken = default)
    {
        if (!Enum.TryParse<Tempest.Protocol.Common.CountryCode>(country, true, out var countryCode))
        {
            countryCode = Tempest.Protocol.Common.CountryCode.Unspecified;
        }

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
            Discover = discover,
            ApiKey = apiKey,
            Country = countryCode,
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

    public async Task Discover(
        int timeoutMs = 1500,
        CancellationToken cancellationToken = default)
    {
        var uniqueKeys = new HashSet<string>();
        using var client = new UdpClient();
        client.EnableBroadcast = true;
        client.Client.Bind(new IPEndPoint(IPAddress.Any, 0));

        var requestBytes = Encoding.UTF8.GetBytes("TEMPEST_DISCOVER");
        
        // 1. Send broadcast on standard 255.255.255.255
        try
        {
            var broadcastEndpoint = new IPEndPoint(IPAddress.Broadcast, 50054);
            await client.SendAsync(requestBytes, requestBytes.Length, broadcastEndpoint);
        }
        catch { }

        // 2. Send broadcast on each active network interface to support virtual networks (Radmin, Hamachi, etc.)
        try
        {
            foreach (var ni in NetworkInterface.GetAllNetworkInterfaces())
            {
                if (ni.OperationalStatus != OperationalStatus.Up) continue;
                if (ni.NetworkInterfaceType == NetworkInterfaceType.Loopback) continue;

                var ipProps = ni.GetIPProperties();
                foreach (var unicast in ipProps.UnicastAddresses)
                {
                    if (unicast.Address.AddressFamily == AddressFamily.InterNetwork)
                    {
                        var ip = unicast.Address.GetAddressBytes();
                        var mask = unicast.IPv4Mask?.GetAddressBytes();
                        if (mask != null && ip.Length == mask.Length)
                        {
                            var broadcastBytes = new byte[ip.Length];
                            for (var i = 0; i < ip.Length; i++)
                            {
                                broadcastBytes[i] = (byte)(ip[i] | ~mask[i]);
                            }
                            var broadcastIp = new IPAddress(broadcastBytes);
                            await client.SendAsync(requestBytes, requestBytes.Length, new IPEndPoint(broadcastIp, 50054));
                        }
                    }
                }
            }
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Interface broadcast error: {ex.Message}");
        }

        using var cts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        cts.CancelAfter(timeoutMs);

        try
        {
            while (!cts.Token.IsCancellationRequested)
            {
                var receiveTask = client.ReceiveAsync(cts.Token);
                var result = await receiveTask;
                
                try
                {
                    var listing = ServerListing.Parser.ParseFrom(result.Buffer);
                    
                    // Override the IP to the sender's actual IP address that we received the response from.
                    // This is essential for routing through virtual VPN layers like Radmin or Hamachi.
                    var actualIp = result.RemoteEndPoint.Address.ToString();
                    listing.Ip = actualIp;
                    
                    var key = actualIp + ":" + listing.LobbyPort;
                    if (uniqueKeys.Add(key))
                    {
                        var json = JsonSerializer.Serialize(listing, ServerListingJsonContext.Default.ServerListing);
                        Console.WriteLine(json);
                        Console.Out.Flush();
                    }
                }
                catch
                {
                    // Ignore malformed packets
                }
            }
        }
        catch (OperationCanceledException)
        {
            // Expected on timeout
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Discovery error: {ex.Message}");
        }
    }
}

[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull)]
[JsonSerializable(typeof(ServerListing))]
[JsonSerializable(typeof(Google.Protobuf.WellKnownTypes.Timestamp))]
internal partial class ServerListingJsonContext : JsonSerializerContext;
