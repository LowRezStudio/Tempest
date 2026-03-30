using Google.Protobuf.WellKnownTypes;
using System.Collections.Concurrent;
using System.Diagnostics;
using Tempest.CLI.Launcher;
using Tempest.Protocol.Lobby;

namespace Tempest.CLI.Server;

internal sealed class LobbyState(LobbyServerOptions options, ITicketStore ticketStore)
{
    private readonly LobbyServerOptions _options = options;
    private readonly ConcurrentDictionary<string, LobbyPlayer> _players = new();
    private readonly ITicketStore _ticketStore = ticketStore;
    private readonly ConcurrentQueue<LobbyEvent> _eventBuffer = new();
    private readonly List<IObserver<LobbyEvent>> _subscribers = new();
    private readonly object _gate = new();
    private Protocol.Lobby.LobbyState _state = new Protocol.Lobby.LobbyState
    {
        Waiting = new LobbyStateWaiting
        {
            MinPlayers = (uint)options.MinPlayers
        }
    };
    private CancellationTokenSource? _countdownCts;
    private LobbyEventCountdown? _countdown;

    public IReadOnlyCollection<LobbyPlayer> Players => _players.Values.ToList();

    public bool TryJoin(string id, string displayName, string? password, out JoinLobbyResponse response)
    {
        if (!string.IsNullOrEmpty(_options.Password) && _options.Password != password)
        {
            response = Error(JoinLobbyErrorCode.InvalidPassword, "Invalid password");
            return false;
        }
        if (_players.Count >= _options.MaxPlayers)
        {
            response = Error(JoinLobbyErrorCode.LobbyFull, "Lobby full");
            return false;
        }
        //simple balancing that puts the new player into the team that has less players
        int team = _players.Values.Count(p => p.TaskForce == 1) > _players.Count / 2.0 ? 2 : 1;
        var player = new LobbyPlayer
        {
            Id = id,
            DisplayName = displayName,
            TaskForce = team,
            Champion = string.Empty
        };

        if (!_players.TryAdd(id, player))
        {
            response = Error(JoinLobbyErrorCode.LobbyInvalid, "Player already in lobby");
            return false;
        }

        Publish(new LobbyEvent
        {
            Timestamp = Timestamp.FromDateTime(DateTime.UtcNow),
            PlayerJoin = new LobbyEventPlayerJoin { Player = player }
        });
        StateMachine();

        var ticket = _ticketStore.Issue(id);
        response = new JoinLobbyResponse { Success = new JoinLobbySuccess { Ticket = ticket } };
        return true;
    }

    public bool TryLeave(string id)
    {
        if (_players.TryRemove(id, out var player))
        {
            _ticketStore.RevokeTickets(id);
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

    public void SendChat(string playerId, string content)
    {
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
        _state.MapVote.Votes[playerId] = mapId;
        PublishState();
        StateMachine();
    }   

    public bool TryGetPlayerIdFromTicket(string ticket, out string playerId) => _ticketStore.TryGetPlayerId(ticket, out playerId);

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
        if (_state.Waiting != null)
        {
            bool enoughPlayers = _players.Count >= _options.MinPlayers;
            if (enoughPlayers && _countdown == null) StartCountdown(10, EndWaiting);
            else if (!enoughPlayers) CancelCountdown();
        }
        else if (_state.MapVote != null)
        {
            bool everyoneHasVoted = _state.MapVote.Votes.Count >= _players.Count && _state.MapVote.Votes.Count > 0;
            bool oneHasVoted = _state.MapVote.Votes.Count > 0;
            if (everyoneHasVoted) StartCountdown(5, EndMapVote);
            else if (_countdown == null && oneHasVoted) StartCountdown(15, EndMapVote);
        }
        else if (_state.ChampionSelect != null)
        {
            bool allHaveSelected = _players.Values.All(p => p.Champion != null && p.Champion.Length > 0);
            bool oneHasSelected = _players.Values.Any(p => p.Champion != null && p.Champion.Length > 0);
            if (allHaveSelected) StartCountdown(5, EndChampionSelect);
            else if (_countdown == null && oneHasSelected) StartCountdown(60, EndChampionSelect);
        }
        else if (_state.InGame != null)
        {
            if (_state.InGame.GameServerFinishedRunning && _countdown == null)
            {
                StartCountdown(10, EndInGame);
            }
        }
    }
    private void EndWaiting()
    {
        SetState(new Protocol.Lobby.LobbyState { MapVote = new LobbyStateMapVote() });
    }

    private void EndMapVote()
    {
        //selecting the most voted map and breaking ties by choosing randomly
        var groups = _state.MapVote.Votes.GroupBy(t => t.Value).ToList();
        int maxVotes = groups.Max(g => g.Count());
        var topMaps = groups.Where(g => g.Count() == maxVotes).ToList();
        string mapId = topMaps[Random.Shared.Next(topMaps.Count)].Key;
        SetState(new Protocol.Lobby.LobbyState { ChampionSelect = new LobbyStateChampionSelect { MapId = mapId } });
    }

    private void EndChampionSelect()
    {
        var mapId = _state.ChampionSelect.MapId;
        SetState(new Protocol.Lobby.LobbyState { InGame = new LobbyStateInGame { } });
        StartGameServer(mapId);
    }
    private void EndInGame()
    {
        foreach (var player in _players.Values.ToList())
        {
            player.Champion = string.Empty;
        }
        //not using SetState because it would cause an unnecessary state update
        if (_players.Count >= _options.MinPlayers)
        {
            _state = new Protocol.Lobby.LobbyState { MapVote = new LobbyStateMapVote() };
        }
        else
        {
            _state = new Protocol.Lobby.LobbyState
            {
                Waiting = new LobbyStateWaiting
                {
                    MinPlayers = (uint)_options.MinPlayers
                }
            };
        }
        StateMachine();
        Publish(GetInfoEvent());
    }

    private async void StartGameServer(string mapId)
    {
        string champions = string.Join(",", _players.Where(p => p.Value.Champion.Length > 0).Select(p => p.Value.Champion.ToLower()));
        //TODO other gamemodes
        string[] args = ["server", $"{mapId}?game=TempestMp.SiegeDEV?allowedChampions={champions}?maxplayers={_options.MaxPlayers}"];
        var process = await LauncherCommands.LaunchGame(_options.Path, args, _options.NoDefaultArgs, _options.Platform, _options.Game, _options.Dll, true);
        SetState(new Protocol.Lobby.LobbyState
        {
            InGame = new LobbyStateInGame
            {
                GameServerOpen = true
            }
        });

        await process.WaitForExitAsync();

        SetState(new Protocol.Lobby.LobbyState
        {
            InGame = new LobbyStateInGame
            {
                GameServerFinishedRunning = true,
                GameServerError = process.ExitCode != 0
            }
        });
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
            Name = _options.Name,
            State = _state,
            Version = _options.Version,
            PasswordRequired = _options.Password != null && !_options.Password.Equals(string.Empty),
            MaxPlayers = (uint) _options.MaxPlayers,
            Countdown = _countdown
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

    private sealed class EventStream : IObservable<LobbyEvent>, IObserver<LobbyEvent>, IDisposable
    {
        private readonly LobbyState _parent;
        private bool _disposed;

        public EventStream(LobbyState parent) => _parent = parent;

        public IDisposable Subscribe(IObserver<LobbyEvent> observer)
        {
            lock (_parent._gate)
            {
                _parent._subscribers.Add(observer);
            }
            return this;
        }

        public void OnCompleted() { }
        public void OnError(Exception error) { }
        public void OnNext(LobbyEvent value) { }

        public void Dispose()
        {
            if (_disposed) return;
            lock (_parent._gate)
            {
                _parent._subscribers.Remove(this);
            }
            _disposed = true;
        }
    }
}
