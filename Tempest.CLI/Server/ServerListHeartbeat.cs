using Grpc.Core;
using System.Net;
using System.Net.Sockets;
using Tempest.Protocol.ServerList;


namespace Tempest.CLI.Server;

internal sealed class ServerListHearthbeat : BackgroundService
{
    private readonly LobbyServerOptions _options;
    private readonly ServerListClient _client;
    private readonly TimeSpan _checkInterval = TimeSpan.FromSeconds(120);
    private string? _ownId = null;
    
    public ServerListHearthbeat(LobbyServerOptions options)
    {
        _options = options;
        var servicesUrl = _options.ServicesUrl ?? "https://localhost:7165";
        _client = new ServerListClient(servicesUrl);
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        while (!stoppingToken.IsCancellationRequested)
        {
            try
            {
                if (await ShouldAddToServerList(stoppingToken))
                {
                    var addResponse = await RegisterWithServerListAsync(stoppingToken);
                    if (addResponse.ResultCase == CreateLobbyResponse.ResultOneofCase.Success)
                    {
                        Console.WriteLine($"Successfully registered with ServerList service. Id: {addResponse.Success.Id}");
                        _ownId = addResponse.Success.Id;
                    }
                    else if (addResponse.ResultCase == CreateLobbyResponse.ResultOneofCase.Error)
                    {
                        Console.WriteLine($"Failed to register with ServerList: {addResponse.Error.Message}");
                    }
                }
            }
            catch (Exception ex)
            {
                Console.WriteLine($"Failed to register with ServerList: {ex.Message}");
            }
            
            await Task.Delay(_checkInterval, stoppingToken);
        }
        _client.Dispose();
    }
    private async Task<bool> ShouldAddToServerList(CancellationToken stoppingToken)
    {
        if (_ownId == null) return true;
        try
        {
            var response = await _client.GetServerByIdAsync(_ownId, stoppingToken);
            return response.ResultCase == GetServerByIdResponse.ResultOneofCase.Error  && response.Error.Code == GetServerByIdErrorCode.IdNotFound;
        } catch (Exception ex)
        {
            Console.WriteLine($"Error fetching serverlisting with id {_ownId}: {ex.Message}");
            return false;
        }
    }

    private async Task<CreateLobbyResponse> RegisterWithServerListAsync(CancellationToken stoppingToken)
    {
        var localIp = GetLocalIpAddress();
        var request = new CreateLobbyRequest
        {
            Ip = localIp,
            LobbyPort = (uint)_options.Port,
            Name = _options.Name,
            Game = "Paladins",
            Version = _options.Version,
            MaxPlayers = (uint)_options.MaxPlayers,
            MaxSpectators = 0,
            JoinInProgress = _options.JoinInProgress,
            Joinable = true,
            HasPassword = !string.IsNullOrEmpty(_options.Password),
            Country = Protocol.Common.CountryCode.Us
        };

        if (!string.IsNullOrEmpty(_options.Map))
            request.Map = _options.Map;

        foreach (var tag in _options.Tags)
        {
            request.Tags.Add(tag);
        }

        return await _client.CreateLobbyAsync(request, stoppingToken);
    }
    private static string GetLocalIpAddress()
    {
        using var socket = new Socket(AddressFamily.InterNetwork, SocketType.Dgram, 0);
        socket.Connect("8.8.8.8", 65530);
        var endPoint = socket.LocalEndPoint as IPEndPoint;
        return endPoint?.Address.ToString() ?? "127.0.0.1";
    }
}