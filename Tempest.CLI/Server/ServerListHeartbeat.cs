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
    private readonly TimeSpan _periodicInterval = TimeSpan.FromSeconds(30);
    private readonly TimeSpan _minHeartbeatInterval = TimeSpan.FromSeconds(10);
    private string? _ownId;
    private string? _ticket;
    private DateTime _lastHeartbeat = DateTime.MinValue;
    private CancellationTokenSource? _periodicCts;
    private readonly Lock _heartbeatLock = new();

    public ServerListHeartbeat(LobbyServerOptions options, LobbyState state, ILogger<ServerListHeartbeat> logger)
    {
        _options = options;
        _state = state;
        _logger = logger;
        _client = new ServerListClient(_options.ServicesUrl ?? "https://api.lowrezstudio.com");
        _state.Subscribe("").Subscribe(new Observer(this));
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        await Task.Yield();
        while (!stoppingToken.IsCancellationRequested)
        {
            // Wait for the periodic interval; event-driven heartbeats can reset this timer
            _periodicCts = CancellationTokenSource.CreateLinkedTokenSource(stoppingToken);
            try
            {
                await Task.Delay(_periodicInterval, _periodicCts.Token);
                // Timer elapsed without being reset — send a periodic heartbeat
                await SendHeartbeatAsync(stoppingToken);
            }
            catch (OperationCanceledException)
            {
                // Timer was reset by an event-driven heartbeat; restart the delay
            }
        }
        _client.Dispose();
    }

    private async Task SendHeartbeatAsync(CancellationToken ct)
    {
        // Rate-limit: ensure at least _minHeartbeatInterval between heartbeats
        lock (_heartbeatLock)
        {
            var now = DateTime.UtcNow;
            if (now - _lastHeartbeat < _minHeartbeatInterval)
                return;
            _lastHeartbeat = now;
        }

        // A heartbeat is being sent — reset the periodic timer so it doesn't fire too soon
        ResetPeriodicTimer();

        try
        {
            if (_ownId == null)
            {
                var response = await RegisterAsync(ct);
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
                var request = BuildUpdateRequest();
                var response = await _client.UpdateLobbyAsync(request, ct);

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
    }

    private UpdateLobbyRequest BuildUpdateRequest()
    {
        var info = _state.GetInfoEvent().Info;
        string? mapId = null;
        if (info.State.ChampionSelect != null)
            mapId = info.State.ChampionSelect.MapId;
        if (info.State.InGame != null)
            mapId = info.State.InGame.MapId;

        var joinable = true;
        if (info.State.InGame != null && !_options.JoinInProgress && !_options.EnableJoinInProgress)
            joinable = false;

        return new UpdateLobbyRequest
        {
            Id = _ownId,
            Ticket = _ticket,
            Players = (uint)info.Players.Count,
            MapId = mapId,
            Joinable = joinable
        };
    }

    private void ResetPeriodicTimer()
    {
        _periodicCts?.Cancel();
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

            // Fire-and-forget: triggers a heartbeat (subject to rate limiting)
            _ = parent.SendHeartbeatAsync(CancellationToken.None);
        }
    }
}
