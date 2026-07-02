using System.Net;
using System.Net.Sockets;
using System.Text;
using Google.Protobuf;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Tempest.Protocol.ServerList;

namespace Tempest.CLI.Server;

internal sealed class LanDiscoveryResponder : BackgroundService
{
    private readonly LobbyServerOptions _options;
    private readonly LobbyState _state;
    private readonly ILogger<LanDiscoveryResponder> _logger;
    private const int DiscoveryPort = 50054;

    public LanDiscoveryResponder(LobbyServerOptions options, LobbyState state, ILogger<LanDiscoveryResponder> logger)
    {
        _options = options;
        _state = state;
        _logger = logger;
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        UdpClient? udpClient = null;
        UdpClient? hijackClient = null;
        try
        {
            udpClient = new UdpClient();
            udpClient.Client.SetSocketOption(SocketOptionLevel.Socket, SocketOptionName.ReuseAddress, true);
            udpClient.Client.Bind(new IPEndPoint(IPAddress.Any, DiscoveryPort));
            _logger.LogInformation("LAN Discovery Responder listening on UDP port {Port}", DiscoveryPort);

            hijackClient = new UdpClient();
            hijackClient.Client.SetSocketOption(SocketOptionLevel.Socket, SocketOptionName.ReuseAddress, true);
            hijackClient.Client.Bind(new IPEndPoint(IPAddress.Any, _options.GameServerPort));
            _logger.LogInformation("LAN Discovery Responder hijacked game server UDP port {Port}", _options.GameServerPort);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to start LAN Discovery Responder");
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
                    if (_state.GetInfoEvent().Info.State.InGame != null)
                    {
                        if (hijackClient.Client != null)
                        {
                            _logger.LogInformation("Game server starting, releasing hijacked port {Port}", _options.GameServerPort);
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
                if (requestText == "TEMPEST_DISCOVER")
                {
                    var responseBytes = CreateResponseBytes();
                    await client.SendAsync(responseBytes, responseBytes.Length, result.RemoteEndPoint);
                }
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
                    _logger.LogError(ex, "Error in LAN Discovery Responder loop");
                }
            }
        }
    }

    private byte[] CreateResponseBytes()
    {
        var localIp = GetLocalIpAddress();
        var info = _state.GetInfoEvent().Info;
        
        string? mapId = null;
        if (info.State.ChampionSelect != null)
            mapId = info.State.ChampionSelect.MapId;
        if (info.State.InGame != null)
            mapId = info.State.InGame.MapId;

        bool joinable = true;
        if (info.State.InGame != null && !_options.JoinInProgress && !_options.EnableJoinInProgress)
        {
            joinable = false;
        }

        var listing = new ServerListing
        {
            Id = $"lan_{_options.Port}_{localIp}",
            Ip = localIp,
            LobbyPort = (uint)_options.Port,
            Name = _options.Name,
            Game = _options.GameMode ?? "",
            Version = _options.Version ?? "",
            Players = (uint)info.Players.Count,
            MaxPlayers = (uint)_options.MaxPlayers,
            Bots = 0,
            MaxSpectators = 0,
            Spectators = 0,
            JoinInProgress = _options.JoinInProgress,
            Joinable = joinable,
            HasPassword = !string.IsNullOrEmpty(_options.Password),
            Country = _options.Country,
        };

        if (!string.IsNullOrEmpty(_options.Map))
        {
            listing.Map = _options.Map;
        }
        else if (!string.IsNullOrEmpty(mapId))
        {
            listing.Map = mapId;
        }

        if (!string.IsNullOrEmpty(mapId))
        {
            listing.MapId = mapId;
        }

        foreach (var tag in _options.Tags)
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
