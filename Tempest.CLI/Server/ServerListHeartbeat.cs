using System.Net;
using System.Net.Sockets;
using Tempest.Protocol.Lobby;
using Tempest.Protocol.ServerList;

namespace Tempest.CLI.Server;

internal sealed class ServerListHeartbeat : BackgroundService
{
    private readonly LobbyServerOptions _options;
    private readonly LobbyState _state;
    private readonly ServerListClient _client;
    private readonly TimeSpan _checkInterval = TimeSpan.FromSeconds(120);
    private string? _ownId;

    public ServerListHeartbeat(LobbyServerOptions options, LobbyState state)
    {
        _options = options;
        _state = state;
        _client = new ServerListClient(_options.ServicesUrl ?? "https://localhost:7165");
        _state.Subscribe().Subscribe(new Observer(this));
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        while (!stoppingToken.IsCancellationRequested)
        {
            try
            {
                if (await ShouldRegisterAsync(stoppingToken))
                {
                    var response = await RegisterAsync(stoppingToken);
                    if (response.ResultCase == CreateLobbyResponse.ResultOneofCase.Success)
                    {
                        Console.WriteLine($"Successfully registered with ServerList service. Id: {response.Success.Id}");
                        _ownId = response.Success.Id;
                    }
                    else if (response.ResultCase == CreateLobbyResponse.ResultOneofCase.Error)
                    {
                        Console.WriteLine($"Failed to register with ServerList: {response.Error.Message}");
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

    private async Task<bool> ShouldRegisterAsync(CancellationToken ct)
    {
        if (_ownId == null) return true;
        try
        {
            var response = await _client.GetServerByIdAsync(_ownId, ct);
            return response.ResultCase == GetServerByIdResponse.ResultOneofCase.Error
                && response.Error.Code == GetServerByIdErrorCode.IdNotFound;
        }
        catch (Exception ex)
        {
            Console.WriteLine($"Error checking server list entry: {ex.Message}");
            return false;
        }
    }

    private async Task<CreateLobbyResponse> RegisterAsync(CancellationToken ct)
    {
        var request = new CreateLobbyRequest
        {
            Ip = GetLocalIpAddress(),
            LobbyPort = (uint)_options.Port,
            Name = _options.Name,
            Game = _options.GameMode,
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
            request.Tags.Add(tag);

        return await _client.CreateLobbyAsync(request, ct);
    }

    private static string GetLocalIpAddress()
    {
        using var socket = new Socket(AddressFamily.InterNetwork, SocketType.Dgram, 0);
        socket.Connect("8.8.8.8", 65530);
        var endPoint = socket.LocalEndPoint as IPEndPoint;
        return endPoint?.Address.ToString() ?? "127.0.0.1";
    }

    private sealed class Observer(ServerListHeartbeat parent) : IObserver<LobbyEvent>
    {
        private readonly ServerListHeartbeat _parent = parent;
        public void OnCompleted() { }
        public void OnError(Exception error) { }

        public void OnNext(LobbyEvent ev)
        {
            if (_parent._ownId == null) return;
            if (ev.PlayerJoin == null && ev.PlayerLeave == null && ev.StateUpdate == null && ev.Info == null) return;

            var info = _parent._state.GetInfoEvent().Info;
            string? mapId = null;
            if (info.State.ChampionSelect != null)
                mapId = info.State.ChampionSelect.MapId;
            if (info.State.InGame != null)
            {
                mapId = info.State.InGame.MapId;
            }

            var request = new UpdateLobbyRequest
            {
                Id = _parent._ownId,
                Players = (uint)info.Players.Count,
                MapId = mapId,
            };
            try
            {
                _ = _parent._client.UpdateLobbyAsync(request);
            }
            catch (Exception ex)
            {
                Console.WriteLine($"Failed to update ServerList entry {ex.Message}");
            }
        }
    }
}
