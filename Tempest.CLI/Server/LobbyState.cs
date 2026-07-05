using Google.Protobuf.WellKnownTypes;
using System.Collections.Concurrent;
using System.Diagnostics;
using Tempest.CLI.Launcher;
using Tempest.Protocol.Lobby;

namespace Tempest.CLI.Server;

internal sealed class LobbyState(LobbyServerOptions options, ITicketStore ticketStore, ILogger<LobbyState> logger)
{
    private readonly ConcurrentDictionary<string, LobbyPlayer> _players = new();
    private readonly ConcurrentQueue<LobbyEvent> _eventBuffer = new();
    private readonly List<IObserver<LobbyEvent>> _subscribers = [];
    private readonly Lock _gate = new();
    private Protocol.Lobby.LobbyState _state = new()
    {
        Waiting = new LobbyStateWaiting
        {
            MinPlayers = (uint)options.MinPlayers
        }
    };
    private CancellationTokenSource? _countdownCts;
    private LobbyEventCountdown? _countdown;
    private Process? _gameProcess;
    private bool _gameServerKilledIntentionally;
    private readonly Lock _stateMachineLock = new();
    private Task? _gameServerTask;

    public bool TryJoin(string id, string displayName, string? password, out JoinLobbyResponse response)
    {
        if (_state.InGame != null && !options.JoinInProgress && !options.EnableJoinInProgress)
        {
            // ponytail: block joining mid-game if join-in-progress is not enabled
            logger.LogWarning("Player {DisplayName} failed to join: game is in progress and join-in-progress is disabled", displayName);
            response = Error(JoinLobbyErrorCode.LobbyInvalid, "Game is already in progress");
            return false;
        }
        if (!string.IsNullOrEmpty(options.Password) && options.Password != password)
        {
            logger.LogWarning("Player {DisplayName} failed to join: invalid password", displayName);
            response = Error(JoinLobbyErrorCode.InvalidPassword, "Invalid password");
            return false;
        }
        if (_players.Count >= options.MaxPlayers)
        {
            logger.LogWarning("Player {DisplayName} failed to join: lobby full ({PlayerCount}/{MaxPlayers})", displayName, _players.Count, options.MaxPlayers);
            response = Error(JoinLobbyErrorCode.LobbyFull, "Lobby full");
            return false;
        }
        //simple balancing that puts the new player into the team that has less players
        var team = _players.Values.Count(p => p.TaskForce == 1) > _players.Count / 2.0 ? 2 : 1;
        var player = new LobbyPlayer
        {
            Id = id,
            DisplayName = displayName,
            TaskForce = team,
            Champion = string.Empty
        };

        if (!_players.TryAdd(id, player))
        {
            logger.LogWarning("Player {DisplayName} failed to join: already in lobby", displayName);
            response = Error(JoinLobbyErrorCode.LobbyInvalid, "Player already in lobby");
            return false;
        }

        logger.LogInformation("Player {DisplayName} joined lobby (team {Team}, {PlayerCount}/{MaxPlayers})", displayName, team, _players.Count, options.MaxPlayers);

        Publish(new LobbyEvent
        {
            Timestamp = Timestamp.FromDateTime(DateTime.UtcNow),
            PlayerJoin = new LobbyEventPlayerJoin { Player = player }
        });
        StateMachine();

        var ticket = ticketStore.Issue(id);
        response = new JoinLobbyResponse { Success = new JoinLobbySuccess { Ticket = ticket } };
        return true;
    }

    public bool TryLeave(string id)
    {
        if (_players.TryRemove(id, out var player))
        {
            logger.LogInformation("Player {DisplayName} left lobby ({PlayerCount}/{MaxPlayers})", player.DisplayName, _players.Count, options.MaxPlayers);
            ticketStore.RevokeTickets(id);
            if (_state.MapVote != null)
            {
                _state.MapVote.Votes.Remove(id);
                PublishState();
            }
            Publish(new LobbyEvent
            {
                Timestamp = Timestamp.FromDateTime(DateTime.UtcNow),
                PlayerLeave = new LobbyEventPlayerLeave { PlayerId = player.Id }
            });
            StateMachine();
            return true;
        }
        return false;
    }
    public bool KickPlayer(string id)
    {
        if (_players.TryGetValue(id, out var player))
        {
            logger.LogInformation("Kicking player {DisplayName} from lobby", player.DisplayName);
        }
        return TryLeave(id);
    }
    public void SendChat(string playerId, string content)
    {
        if (_players.TryGetValue(playerId, out var player))
        {
            logger.LogDebug("Chat message from {DisplayName}: {Content}", player.DisplayName, content);
        }
        var evt = new LobbyEvent
        {
            Timestamp = Timestamp.FromDateTime(DateTime.UtcNow),
            ChatMessage = new LobbyEventChatMessage
            {
                ChatMessage = new LobbyChatMessage
                {
                    AuthorId = playerId,
                    Content = content,
                    SentAt = Timestamp.FromDateTime(DateTime.UtcNow)
                }
            }
        };
        Publish(evt);
    }

    public bool TrySelectChampion(string playerId, string champion)
    {
        if (!_players.TryGetValue(playerId, out var player)) return false;
        logger.LogInformation("Player {DisplayName} selected champion {Champion}", player.DisplayName, champion);
        player.Champion = champion;
        Publish(new LobbyEvent
        {
            Timestamp = Timestamp.FromDateTime(DateTime.UtcNow),
            PlayerUpdate = new LobbyEventPlayerUpdate { Player = player }
        });
        StateMachine();
        return true;
    }
    public void Vote(string playerId, string mapId)
    {
        if (_state.MapVote == null) return;
        if (_players.TryGetValue(playerId, out var player))
        {
            logger.LogInformation("Player {DisplayName} voted for map {MapId}", player.DisplayName, mapId);
        }
        _state.MapVote.Votes[playerId] = mapId;
        PublishState();
        StateMachine();
    }

    public bool TryGetPlayerIdFromTicket(string ticket, out string playerId) => ticketStore.TryGetPlayerId(ticket, out playerId);

    public bool TryLeaveByTicket(string ticket)
    {
        if (TryGetPlayerIdFromTicket(ticket, out var playerId))
        {
            return TryLeave(playerId);
        }
        return false;
    }
    //this method should be called when state is modified.
    private void StateMachine()
    {
        lock (_stateMachineLock)
        {
            if (_state.Waiting != null)
            {
                var enoughPlayers = _players.Count >= options.MinPlayers;
                if (enoughPlayers && _countdown == null) StartCountdown(10, EndWaiting);
                else if (!enoughPlayers) CancelCountdown();
            }
            else if (_state.MapVote != null)
            {
                var everyoneHasVoted = _state.MapVote.Votes.Count >= _players.Count && _state.MapVote.Votes.Count > 0;
                var oneHasVoted = _state.MapVote.Votes.Count > 0;
                if (everyoneHasVoted) StartCountdown(5, EndMapVote);
                else if (_countdown == null && oneHasVoted) StartCountdown(15, EndMapVote);
            }
            else if (_state.ChampionSelect != null)
            {
                var allHaveSelected = _players.Values.All(p => p.Champion != null && p.Champion.Length > 0);
                var oneHasSelected = _players.Values.Any(p => p.Champion != null && p.Champion.Length > 0);
                if (allHaveSelected) StartCountdown(5, EndChampionSelect);
                else if (_countdown == null && oneHasSelected) StartCountdown(60, EndChampionSelect);
            }
            else if (_state.InGame != null)
            {
                var everyoneHasLeft = _players.Values.All(p => p.Champion == null || p.Champion.Length == 0);
                if (everyoneHasLeft && !_state.InGame.GameServerFinishedRunning)
                    KillGameServer();
                if (_state.InGame.GameServerFinishedRunning && _countdown == null)
                    StartCountdown(10, EndInGame);
            }
        }
    }
    private void EndWaiting()
    {
        logger.LogInformation("Starting map vote");
        SetState(new Protocol.Lobby.LobbyState { MapVote = new LobbyStateMapVote() });
    }

    private void EndMapVote()
    {
        //selecting the most voted map and breaking ties by choosing randomly
        var groups = _state.MapVote.Votes.GroupBy(t => t.Value).ToList();
        var maxVotes = groups.Max(g => g.Count());
        var topMaps = groups.Where(g => g.Count() == maxVotes).ToList();
        var mapId = topMaps[Random.Shared.Next(topMaps.Count)].Key;
        logger.LogInformation("Map vote ended, selected map {MapId} with {VoteCount} votes", mapId, maxVotes);
        SetState(new Protocol.Lobby.LobbyState { ChampionSelect = new LobbyStateChampionSelect { MapId = mapId } });
    }

    private void EndChampionSelect()
    {
        var mapId = _state.ChampionSelect.MapId;
        foreach (var player in _players.Values)
        {
            if (player.Champion == null || player.Champion.Length == 0)
            {
                logger.LogInformation("Player {DisplayName} did not select a champion and was kicked", player.DisplayName);
                KickPlayer(player.Id);
            }
        }
        logger.LogInformation("Champion select ended, starting game server on map {MapId} with {PlayerCount} players", mapId, _players.Count);
        SetState(new Protocol.Lobby.LobbyState
        {
            InGame = new LobbyStateInGame
            {
                MapId = mapId,
                GameServerPort = (uint)options.GameServerPort,
            }
        });
        StartGameServer(mapId);
    }
    private void EndInGame()
    {
        foreach (var player in _players.Values.ToList())
        {
            player.Champion = string.Empty;
        }
        //not using SetState because it would cause an unnecessary state update
        if (_players.Count >= options.MinPlayers)
        {
            _state = new Protocol.Lobby.LobbyState { MapVote = new LobbyStateMapVote() };
        }
        else
        {
            _state = new Protocol.Lobby.LobbyState
            {
                Waiting = new LobbyStateWaiting
                {
                    MinPlayers = (uint)options.MinPlayers
                }
            };
        }
        StateMachine();
        Publish(GetInfoEvent());
    }

    private Task StartGameServer(string mapId)
    {
        _gameServerTask = RunGameServerAsync(mapId);
        return _gameServerTask;
    }

    private async Task RunGameServerAsync(string mapId)
    {
        var champions = string.Join(",", _players.Where(p => p.Value.Champion.Length > 0).Select(p => p.Value.Champion.ToLower()));
        var serverArgs = $"{mapId}?game={options.GameMode}?allowedChampions={champions},maldamba?maxplayers={options.MaxPlayers}";
        if (options.Password != null && options.Password.Length > 0)
        {
            serverArgs += $"?password={options.Password}";
        }
        string[] args = ["server", serverArgs, $"-port={options.GameServerPort}"];
        logger.LogInformation("Launching game server with args: {ServerArgs} -port={Port}", serverArgs, options.GameServerPort);
        var process = await LauncherCommands.LaunchGame(options.Path, args, options.NoDefaultArgs, options.Platform, options.Game, options.Dll, true);
        _gameProcess = process;
        logger.LogInformation("Game server started (PID {ProcessId})", process.Id);
        SetState(new Protocol.Lobby.LobbyState
        {
            InGame = new LobbyStateInGame
            {
                GameServerOpen = true,
                MapId = mapId,
                GameServerPort = (uint)options.GameServerPort,
            }
        });

        await process.WaitForExitAsync();
        _gameProcess = null;

        var gameServerError = process.ExitCode != 0 && !_gameServerKilledIntentionally;
        logger.LogInformation("Game server exited with code {ExitCode} (killed intentionally: {KilledIntentionally}, error: {Error})", process.ExitCode, _gameServerKilledIntentionally, gameServerError);
        SetState(new Protocol.Lobby.LobbyState
        {
            InGame = new LobbyStateInGame
            {
                GameServerFinishedRunning = true,
                GameServerError = gameServerError,
                MapId = mapId,
                GameServerPort = (uint)options.GameServerPort,
            }
        });
        _gameServerKilledIntentionally = false;
    }

    public void KillGameServer()
    {
        var process = _gameProcess;
        if (process == null || process.HasExited) return;
        _gameServerKilledIntentionally = true;
        logger.LogInformation("Killing game server (PID {ProcessId})", process.Id);
        try { process.Kill(true); }
        catch (Exception ex) { logger.LogError(ex, "Failed to kill game server"); }
    }

    private void SetState(Protocol.Lobby.LobbyState state)
    {
        _state = state;
        PublishState();
        StateMachine();
    }

    private void CancelCountdown()
    {
        if (_countdown == null) return;
        logger.LogInformation("Countdown cancelled");
        //letting clients know that countdown has ended
        Publish(new LobbyEvent
        {
            Timestamp = Timestamp.FromDateTime(DateTime.UtcNow),
            Countdown = new LobbyEventCountdown
            {
                StartTime = Timestamp.FromDateTime(DateTime.UtcNow),
                Seconds = 0
            }
        });
        _countdownCts?.Cancel();
        _countdown = null;
    }

    private Task StartCountdown(uint seconds, Action? onExpired = null)
    {
        _countdownCts?.Cancel();
        var cts = new CancellationTokenSource();
        _countdownCts = cts;
        _countdown = new LobbyEventCountdown
        {
            StartTime = Timestamp.FromDateTime(DateTime.UtcNow),
            Seconds = seconds
        };
        logger.LogInformation("Starting countdown: {Seconds}s", seconds);
        Publish(new LobbyEvent { Timestamp = Timestamp.FromDateTime(DateTime.UtcNow), Countdown = _countdown });
        return RunCountdownAsync(seconds, onExpired, cts.Token);
    }

    private async Task RunCountdownAsync(uint seconds, Action? onExpired, CancellationToken ct)
    {
        try { await Task.Delay(TimeSpan.FromSeconds(seconds), ct); }
        catch (OperationCanceledException)
        {
            _countdown = null;
            return;
        }
        _countdown = null;
        onExpired?.Invoke();
    }

    private void PublishState()
    {
        Publish(new LobbyEvent
        {
            Timestamp = Timestamp.FromDateTime(DateTime.UtcNow),
            StateUpdate = new LobbyEventStateUpdate
            {
                State = _state
            }
        });
    }

    public LobbyEvent GetInfoEvent()
    {
        var info = new LobbyEventInfo
        {
            Name = options.Name ?? "",
            State = _state,
            Version = options.Version ?? "",
            PasswordRequired = options.Password != null && !options.Password.Equals(string.Empty),
            MaxPlayers = (uint)options.MaxPlayers,
            Countdown = _countdown,
            Gamemode = options.GameMode ?? "",
            Game = "Paladins",
            EnableJoinInProgress = options.EnableJoinInProgress,
        };
        info.Players.AddRange(_players.Values);
        return new LobbyEvent
        {
            Info = info,
        };
    }

    private static JoinLobbyResponse Error(JoinLobbyErrorCode code, string message) => new()
    {
        Error = new JoinLobbyError
        {
            Code = code,
            Message = message
        }
    };

    private void Publish(LobbyEvent evt)
    {
        lock (_gate)
        {
            _eventBuffer.Enqueue(evt);
            foreach (var s in _subscribers.ToArray())
            {
                try { s.OnNext(evt); } catch { /* ignore */ }
            }
        }
    }

    public IObservable<LobbyEvent> Subscribe() => new EventStream(this);

    private sealed class EventStream(LobbyState parent) : IObservable<LobbyEvent>, IObserver<LobbyEvent>, IDisposable
    {
        private bool _disposed;

        public IDisposable Subscribe(IObserver<LobbyEvent> observer)
        {
            lock (parent._gate)
            {
                parent._subscribers.Add(observer);
            }
            return this;
        }

        public void OnCompleted() { }
        public void OnError(Exception error) { }
        public void OnNext(LobbyEvent value) { }

        public void Dispose()
        {
            if (_disposed) return;
            lock (parent._gate)
            {
                parent._subscribers.Remove(this);
            }
            _disposed = true;
        }
    }
}
