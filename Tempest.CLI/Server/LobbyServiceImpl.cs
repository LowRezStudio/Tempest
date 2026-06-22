using Grpc.Core;
using System.Threading.Channels;
using Tempest.Protocol.Common;
using Tempest.Protocol.Lobby;

namespace Tempest.CLI.Server;

internal sealed class LobbyServiceImpl(LobbyState state, ITicketStore ticketStore, PlayerDisconnectMonitor playerDisconnectMonitor, LobbyServerOptions options) : Lobby.LobbyBase
{
    private readonly LobbyState _state = state;
    private readonly ITicketStore _ticketStore = ticketStore;
    private readonly PlayerDisconnectMonitor _playerDisconnectMonitor = playerDisconnectMonitor;
    private readonly LobbyServerOptions _options = options;
    private const string TicketHeader = "x-ticket";

    public override Task<JoinLobbyResponse> JoinLobby(JoinLobbyRequest request, ServerCallContext context)
    {
        if (request.AuthMethod == AuthMethod.Ticket)
        {
            return Task.FromResult(new JoinLobbyResponse
            {
                Error = new JoinLobbyError
                {
                    Code = JoinLobbyErrorCode.LobbyInvalid,
                    Message = "Ticket auth is not implemented yet"
                }
            });
        }

        if (string.IsNullOrWhiteSpace(request.AuthValue))
        {
            return Task.FromResult(new JoinLobbyResponse
            {
                Error = new JoinLobbyError
                {
                    Code = JoinLobbyErrorCode.LobbyInvalid,
                    Message = "Missing username"
                }
            });
        }

        var playerId = Guid.NewGuid().ToString();
        _state.TryJoin(playerId, request.AuthValue, request.Password, out var response);
        if (response.Success != null)
        {
            response.Success.PlayerId = playerId;
        }
        return Task.FromResult(response);
    }

    public override Task<LeaveLobbyResponse> LeaveLobby(LeaveLobbyRequest request, ServerCallContext context)
    {
        if (TryGetPlayerId(context, out var playerId))
        {
            _state.TryLeave(playerId);
        }
        return Task.FromResult(new LeaveLobbyResponse());
    }

    public override Task<GetInfoResponse> GetInfo(GetInfoRequest request, ServerCallContext context)
    {
        var info = _state.GetInfoEvent().Info;

        var response = new GetInfoResponse
        {
            Name = _options.Name,
            Game = _options.GameMode ?? string.Empty,
            Version = _options.Version,
            Players = (uint)info.Players.Count,
            MaxPlayers = (uint)_options.MaxPlayers,
            HasPassword = !string.IsNullOrEmpty(_options.Password),
        };

        response.AuthMethods.AddRange(_options.AuthMethods);

        return Task.FromResult(response);
    }

    public override async Task ReceiveLobbyEvents(ReceiveLobbyEventsRequest request, IServerStreamWriter<LobbyEvent> responseStream, ServerCallContext context)
    {
        // Simple subscription that relays events to the stream.
        var channel = Channel.CreateBounded<LobbyEvent>(100);
        var subscription = _state.Subscribe().Subscribe(new Observer(channel));
        _playerDisconnectMonitor.PlayerConnected(request.PlayerId);
        try
        {
            //writing the initial data so the client is up to date
            await responseStream.WriteAsync(_state.GetInfoEvent(), context.CancellationToken);
            while (await channel.Reader.WaitToReadAsync(context.CancellationToken))
            {
                while (channel.Reader.TryRead(out var evt))
                    await responseStream.WriteAsync(evt, context.CancellationToken);
            }
            Console.WriteLine($"Event stream terminated!");
        }
        catch (OperationCanceledException) { }
        finally
        {
            _playerDisconnectMonitor.PlayerDisconnected(request.PlayerId);
            subscription.Dispose();
        }
    }

    public override Task<ChampionSelectResponse> ChampionSelect(ChampionSelectRequest request, ServerCallContext context)
    {
        if (!string.IsNullOrWhiteSpace(request.Name) && TryGetPlayerId(context, out var playerId))
        {
            _state.TrySelectChampion(playerId, request.Name);
        }
        return Task.FromResult(new ChampionSelectResponse());
    }

    public override Task<MapVoteResponse> MapVote(MapVoteRequest request, ServerCallContext context)
    {
        if (TryGetPlayerId(context, out var playerId))
        {
            _state.Vote(playerId, request.MapId);
        }
        return Task.FromResult(new MapVoteResponse());
    }

    public override Task<SendChatMessageResponse> SendChatMessage(SendChatMessageRequest request, ServerCallContext context)
    {
        if (!string.IsNullOrWhiteSpace(request.Content) && TryGetPlayerId(context, out var playerId))
        {
            _state.SendChat(playerId, request.Content);
        }
        return Task.FromResult(new SendChatMessageResponse());
    }

    private bool TryGetPlayerId(ServerCallContext context, out string playerId)
    {
        var ticket = context.RequestHeaders.FirstOrDefault(h => h.Key == TicketHeader)?.Value;
        if (!string.IsNullOrWhiteSpace(ticket) && _ticketStore.TryGetPlayerId(ticket!, out playerId!))
        {
            return true;
        }
        playerId = string.Empty;
        return false;
    }

    private sealed class Observer(Channel<LobbyEvent> channel) : IObserver<LobbyEvent>
    {
        private readonly Channel<LobbyEvent> _channel = channel;
        public void OnCompleted() { }
        public void OnError(Exception error) { }
        public void OnNext(LobbyEvent value)
        {
            _channel.Writer.TryWrite(value);
        }
    }
}
