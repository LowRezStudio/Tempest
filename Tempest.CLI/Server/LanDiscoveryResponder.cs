using System.Net;
using System.Net.Sockets;
using System.Text;
using Google.Protobuf;
using Tempest.Protocol.ServerList;

namespace Tempest.CLI.Server;

internal sealed class LanDiscoveryResponder(
    LobbyServerOptions options,
    LobbyState state,
    ILogger<LanDiscoveryResponder> logger)
    : BackgroundService
{
    private const int DiscoveryPort = 50054;

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        UdpClient? udpClient = null;
        UdpClient? hijackClient = null;
        try
        {
            udpClient = new UdpClient();
            udpClient.Client.SetSocketOption(SocketOptionLevel.Socket, SocketOptionName.ReuseAddress, true);
            udpClient.Client.Bind(new IPEndPoint(IPAddress.Any, DiscoveryPort));
            logger.LogInformation("LAN Discovery Responder listening on UDP port {Port}", DiscoveryPort);

            hijackClient = new UdpClient();
            hijackClient.Client.SetSocketOption(SocketOptionLevel.Socket, SocketOptionName.ReuseAddress, true);
            hijackClient.Client.Bind(new IPEndPoint(IPAddress.Any, options.GameServerPort));
            logger.LogInformation("LAN Discovery Responder hijacked game server UDP port {Port}", options.GameServerPort);
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "Failed to start LAN Discovery Responder");
            udpClient?.Dispose();
            hijackClient?.Dispose();
            return;
        }

        using (udpClient)
        using (hijackClient)
        {
            var udpTask = ReceiveLoop(udpClient, stoppingToken);
            var hijackTask = ReceiveLoop(hijackClient, stoppingToken);

            _ = Task.Run(async () =>
            {
                while (!stoppingToken.IsCancellationRequested)
                {
                    if (state.GetInfoEvent().Info.State.InGame != null)
                    {
                        if (hijackClient != null)
                        {
                            logger.LogInformation("Game server starting, releasing hijacked port {Port}", options.GameServerPort);
                            hijackClient.Dispose();
                        }
                        break;
                    }
                    await Task.Delay(1000, stoppingToken);
                }
            }, stoppingToken);

            await Task.WhenAll(udpTask, hijackTask);
        }
    }

    private async Task ReceiveLoop(UdpClient client, CancellationToken stoppingToken)
    {
        while (!stoppingToken.IsCancellationRequested)
        {
            try
            {
                var result = await client.ReceiveAsync(stoppingToken);
                var requestText = Encoding.UTF8.GetString(result.Buffer);
                
                if (requestText != "TEMPEST_DISCOVER") continue;
                
                var responseBytes = CreateResponseBytes();
                await client.SendAsync(responseBytes, responseBytes.Length, result.RemoteEndPoint);
            }
            catch (ObjectDisposedException)
            {
                break;
            }
            catch (SocketException)
            {
                break;
            }
            catch (OperationCanceledException)
            {
                break;
            }
            catch (Exception ex)
            {
                if (!stoppingToken.IsCancellationRequested)
                {
                    logger.LogError(ex, "Error in LAN Discovery Responder loop");
                }
            }
        }
    }

    private byte[] CreateResponseBytes()
    {
        var localIp = GetLocalIpAddress();
        var info = state.GetInfoEvent().Info;
        
        string? mapId = null;
        if (info.State.ChampionSelect != null)
            mapId = info.State.ChampionSelect.MapId;
        if (info.State.InGame != null)
            mapId = info.State.InGame.MapId;

        var joinable = true;
        if (info.State.InGame != null && !options.JoinInProgress && !options.EnableJoinInProgress)
        {
            joinable = false;
        }

        var listing = new ServerListing
        {
            Id = $"lan_{options.Port}_{localIp}",
            Ip = localIp,
            LobbyPort = (uint)options.Port,
            Name = options.Name,
            Game = options.GameMode ?? "",
            Version = options.Version ?? "",
            Players = (uint)info.Players.Count,
            MaxPlayers = (uint)options.MaxPlayers,
            Bots = 0,
            MaxSpectators = 0,
            Spectators = 0,
            JoinInProgress = options.JoinInProgress,
            Joinable = joinable,
            HasPassword = !string.IsNullOrEmpty(options.Password),
            Country = options.Country,
        };

        if (!string.IsNullOrEmpty(options.Map))
        {
            listing.Map = options.Map;
        }
        else if (!string.IsNullOrEmpty(mapId))
        {
            listing.Map = mapId;
        }

        if (!string.IsNullOrEmpty(mapId))
        {
            listing.MapId = mapId;
        }

        foreach (var tag in options.Tags)
        {
            listing.Tags.Add(tag);
        }

        return listing.ToByteArray();
    }

    private string GetLocalIpAddress()
    {
        try
        {
            using var socket = new Socket(AddressFamily.InterNetwork, SocketType.Dgram, 0);
            socket.Connect("8.8.8.8", 65530);
            var endPoint = socket.LocalEndPoint as IPEndPoint;
            if (endPoint?.Address != null)
            {
                return endPoint.Address.ToString();
            }
        }
        catch
        {
            // fallback
        }

        try
        {
            foreach (var ip in Dns.GetHostAddresses(Dns.GetHostName()))
            {
                if (ip.AddressFamily == AddressFamily.InterNetwork && !IPAddress.IsLoopback(ip))
                {
                    return ip.ToString();
                }
            }
        }
        catch
        {
            // fallback
        }

        return "127.0.0.1";
    }
}
