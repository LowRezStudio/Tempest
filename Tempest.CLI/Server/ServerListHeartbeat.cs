using System.Net;
using System.Net.Sockets;
using Grpc.Core;
using Tempest.Protocol.Lobby;
using Tempest.Protocol.ServerList;

namespace Tempest.CLI.Server;

internal sealed class ServerListHeartbeat : BackgroundService
{
    private readonly LobbyServerOptions _options;
    private readonly LobbyState _state;
    private readonly ServerListClient _client;
    private readonly ILogger<ServerListHeartbeat> _logger;
    private readonly TimeSpan _heartbeatInterval = TimeSpan.FromSeconds(30);
    private string? _ownId;
    private string? _ticket;

    public ServerListHeartbeat(LobbyServerOptions options, LobbyState state, ILogger<ServerListHeartbeat> logger)
    {
        _options = options;
        _state = state;
        _logger = logger;
        _client = new ServerListClient(_options.ServicesUrl ?? "https://api.lowrezstudio.com");
        _state.Subscribe().Subscribe(new Observer(this));
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        await Task.Yield();
        while (!stoppingToken.IsCancellationRequested)
        {
            try
            {
                if (_ownId == null)
                {
                    var response = await RegisterAsync(stoppingToken);
                    if (response.ResultCase == CreateLobbyResponse.ResultOneofCase.Success)
                    {
                        _logger.LogInformation("Successfully registered with ServerList service. Id: {ServerListId}", response.Success.Id);
                        _ownId = response.Success.Id;
                        _ticket = response.Success.Ticket;
                    }
                    else if (response.ResultCase == CreateLobbyResponse.ResultOneofCase.Error)
                    {
                        _logger.LogError("Failed to register with ServerList: {ErrorMessage}", response.Error.Message);
                    }
                }
                else
                {
                    var response = await _client.UpdateLobbyAsync(
                        new UpdateLobbyRequest { Id = _ownId, Ticket = _ticket },
                        stoppingToken);

                    if (response.ResultCase == UpdateLobbyResponse.ResultOneofCase.Error)
                    {
                        _logger.LogWarning("Server list heartbeat rejected ({ErrorCode}), re-registering...", response.Error.Code);
                        _ownId = null;
                        _ticket = null;
                    }
                }
            }
            catch (RpcException ex) when (ex.StatusCode == StatusCode.PermissionDenied)
            {
                _logger.LogError("Failed to register with ServerList: access denied (HTTP 403)");
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "Server list heartbeat error");
            }

            await Task.Delay(_heartbeatInterval, stoppingToken);
        }
        _client.Dispose();
    }

    private async Task<CreateLobbyResponse> RegisterAsync(CancellationToken ct)
    {
        var request = new CreateLobbyRequest
        {
            LobbyPort = (uint)_options.Port,
            Name = _options.Name,
            Gamemode = _options.GameMode ?? "",
            Game = "Paladins",
            Version = _options.Version ?? "",
            MaxPlayers = (uint)_options.MaxPlayers,
            MaxSpectators = 0,
            JoinInProgress = _options.JoinInProgress,
            Joinable = true,
            HasPassword = !string.IsNullOrEmpty(_options.Password),
            Country = _options.Country,
            ApiKey = _options.ApiKey ?? string.Empty
        };

        if (!string.IsNullOrEmpty(_options.Map))
            request.Map = _options.Map;

        foreach (var tag in _options.Tags)
            request.Tags.Add(tag);

        return await _client.CreateLobbyAsync(request, ct);
    }

    private sealed class Observer(ServerListHeartbeat parent) : IObserver<LobbyEvent>
    {
        public void OnCompleted() { }
        public void OnError(Exception error) { }

        public void OnNext(LobbyEvent ev)
        {
            if (parent._ownId == null || parent._ticket == null) return;
            if (ev.PlayerJoin == null && ev.PlayerLeave == null && ev.StateUpdate == null && ev.Info == null) return;

            var info = parent._state.GetInfoEvent().Info;
            string? mapId = null;
            if (info.State.ChampionSelect != null)
                mapId = info.State.ChampionSelect.MapId;
            if (info.State.InGame != null)
            {
                mapId = info.State.InGame.MapId;
            }

            var joinable = true;
            if (info.State.InGame != null && !parent._options.JoinInProgress && !parent._options.EnableJoinInProgress)
            {
                // ponytail: hide server from list once started if join-in-progress is disabled
                joinable = false;
            }

            var request = new UpdateLobbyRequest
            {
                Id = parent._ownId,
                Ticket = parent._ticket,
                Players = (uint)info.Players.Count,
                MapId = mapId,
                Joinable = joinable
            };
            try
            {
                _ = parent._client.UpdateLobbyAsync(request);
            }
            catch (Exception ex)
            {
                parent._logger.LogError(ex, "Failed to update ServerList entry");
            }
        }
    }
}
