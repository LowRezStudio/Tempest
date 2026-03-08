using System.Collections.Concurrent;
using Google.Protobuf.WellKnownTypes;
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
        //for now starting the map voting when the minimum number of players have joined
        //A countdown should be implemented in the future
        if (_players.Count >= _options.MinPlayers && _state.Waiting != null)
        {
            SetState(new Protocol.Lobby.LobbyState
            {
                MapVote = new LobbyStateMapVote()
            });
        }

        var ticket = _ticketStore.Issue(id);
        response = new JoinLobbyResponse { Success = new JoinLobbySuccess { Ticket = ticket } };
        return true;
    }

    public bool TryLeave(string id)
    {
        if (_players.TryRemove(id, out var player))
        {
            _ticketStore.RevokeTickets(id);
            //removing map vote
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
        //Starting the game when all players have selected a champion
        //A countdown should be implemented in the future
        bool allHaveSelected = _players.Values.All(p => p.Champion != null && p.Champion.Length > 0);
        if (allHaveSelected)
        {
            SetState(new Protocol.Lobby.LobbyState
            {
                InGame = new LobbyStateInGame()
            });
        } 
        return true;
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

    public void Vote(string playerId, string mapId)
    {
        if (_state.MapVote == null) return;
        _state.MapVote.Votes[playerId] = mapId;
        bool everyoneHasVoted = _state.MapVote.Votes.Count >= _players.Count && _state.MapVote.Votes.Count > 0;
        if (everyoneHasVoted)
        {
            string mostVotedMap = _state.MapVote.Votes.GroupBy(t => t.Value).OrderByDescending(g => g.Count()).First().Key;
            SetState(new Protocol.Lobby.LobbyState
            {
                ChampionSelect = new LobbyStateChampionSelect
                {
                    MapId = mostVotedMap
                }
            });
        }
        PublishState();
    }
    private void SetState(Protocol.Lobby.LobbyState state)
    {
        _state = state;
        PublishState();
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
            State = _state
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
