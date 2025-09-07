using Grpc.Core;
using Tempest.Protocol.Lobby;

namespace Tempest.CLI.Server;

internal sealed class LobbyServiceImpl(LobbyState state, ITicketStore ticketStore) : Lobby.LobbyBase
{
    private readonly LobbyState _state = state;
    private readonly ITicketStore _ticketStore = ticketStore;
    private const string TicketHeader = "x-ticket";

    public override Task<JoinLobbyResponse> JoinLobby(JoinLobbyRequest request, ServerCallContext context)
    {
        if (string.IsNullOrWhiteSpace(request.PlayerId) || string.IsNullOrWhiteSpace(request.PlayerDisplayName))
        {
            return Task.FromResult(new JoinLobbyResponse
            {
                Error = new JoinLobbyError
                {
                    Code = JoinLobbyErrorCode.LobbyInvalid,
                    Message = "Missing player id or display name"
                }
            });
        }
        _state.TryJoin(request.PlayerId, request.PlayerDisplayName, request.Password, out var response);
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

    public override async Task ReceiveLobbyEvents(ReceiveLobbyEventsRequest request, IServerStreamWriter<LobbyEvent> responseStream, ServerCallContext context)
    {
        // Simple subscription that relays events to the stream.
        var queue = new Channel<LobbyEvent>(capacity: 100);
        var subscription = _state.Subscribe().Subscribe(new Observer(queue));
        try
        {
            while (!context.CancellationToken.IsCancellationRequested && await queue.Reader.WaitToReadAsync(context.CancellationToken))
            {
                while (queue.Reader.TryRead(out var evt))
                {
                    await responseStream.WriteAsync(evt);
                }
            }
        }
        catch (OperationCanceledException) { }
        finally
        {
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
        // Map vote could transition lobby state; publish placeholder state update for now.
        if (TryGetPlayerId(context, out _))
        {
            _state.PublishMapVoteState();
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

    private sealed class Observer : IObserver<LobbyEvent>
    {
        private readonly Channel<LobbyEvent> _channel;
        public Observer(Channel<LobbyEvent> channel) => _channel = channel;
        public void OnCompleted() { }
        public void OnError(Exception error) { }
        public void OnNext(LobbyEvent value)
        {
            _channel.Writer.TryWrite(value);
        }
    }
}
