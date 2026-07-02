using Grpc.Core;
using System.Threading.Channels;
using Tempest.Protocol.Common;
using Tempest.Protocol.Lobby;

namespace Tempest.CLI.Server;

internal sealed class LobbyServiceImpl(LobbyState state, ITicketStore ticketStore, PlayerDisconnectMonitor playerDisconnectMonitor, LobbyServerOptions options, ILogger<LobbyServiceImpl> logger) : Lobby.LobbyBase
{
    private const string TicketHeader = "x-ticket";

    public override Task<JoinLobbyResponse> JoinLobby(JoinLobbyRequest request, ServerCallContext context)
    {
        logger.LogInformation("JoinLobby request from {AuthValue} using {AuthMethod}", request.AuthValue, request.AuthMethod);

        if (request.AuthMethod == AuthMethod.Ticket)
        {
            logger.LogWarning("JoinLobby failed for {AuthValue}: ticket auth not implemented", request.AuthValue);
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
            logger.LogWarning("JoinLobby failed: missing username");
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
        var joined = state.TryJoin(playerId, request.AuthValue, request.Password, out var response);
        if (joined && response.Success != null)
        {
            response.Success.PlayerId = playerId;
        }
        else if (response.Error != null)
        {
            logger.LogWarning("JoinLobby failed for {AuthValue}: {ErrorCode} - {ErrorMessage}", request.AuthValue, response.Error.Code, response.Error.Message);
        }
        return Task.FromResult(response);
    }

    public override Task<LeaveLobbyResponse> LeaveLobby(LeaveLobbyRequest request, ServerCallContext context)
    {
        if (TryGetPlayerId(context, out var playerId))
        {
            logger.LogInformation("Player {PlayerId} leaving lobby", playerId);
            state.TryLeave(playerId);
        }
        else
        {
            logger.LogWarning("LeaveLobby failed: missing or invalid ticket");
        }
        return Task.FromResult(new LeaveLobbyResponse());
    }

    public override Task<GetInfoResponse> GetInfo(GetInfoRequest request, ServerCallContext context)
    {
        var info = state.GetInfoEvent().Info;

        var response = new GetInfoResponse
        {
            Name = options.Name,
            Game = options.GameMode ?? string.Empty,
            Version = options.Version,
            Players = (uint)info.Players.Count,
            MaxPlayers = (uint)options.MaxPlayers,
            HasPassword = !string.IsNullOrEmpty(options.Password),
        };

        response.AuthMethods.AddRange(options.AuthMethods);

        return Task.FromResult(response);
    }

    public override async Task ReceiveLobbyEvents(ReceiveLobbyEventsRequest request, IServerStreamWriter<LobbyEvent> responseStream, ServerCallContext context)
    {
        // Simple subscription that relays events to the stream.
        var channel = Channel.CreateBounded<LobbyEvent>(100);
        var subscription = state.Subscribe().Subscribe(new Observer(channel));
        logger.LogInformation("Player {PlayerId} connected to event stream", request.PlayerId);
        playerDisconnectMonitor.PlayerConnected(request.PlayerId);
        try
        {
            //writing the initial data so the client is up to date
            await responseStream.WriteAsync(state.GetInfoEvent(), context.CancellationToken);
            while (await channel.Reader.WaitToReadAsync(context.CancellationToken))
            {
                while (channel.Reader.TryRead(out var evt))
                    await responseStream.WriteAsync(evt, context.CancellationToken);
            }
            logger.LogInformation("Player {PlayerId} event stream terminated normally", request.PlayerId);
        }
        catch (OperationCanceledException)
        {
            logger.LogInformation("Player {PlayerId} event stream cancelled", request.PlayerId);
        }
        finally
        {
            playerDisconnectMonitor.PlayerDisconnected(request.PlayerId);
            subscription.Dispose();
        }
    }

    public override Task<ChampionSelectResponse> ChampionSelect(ChampionSelectRequest request, ServerCallContext context)
    {
        if (!string.IsNullOrWhiteSpace(request.Name) && TryGetPlayerId(context, out var playerId))
        {
            logger.LogInformation("Player {PlayerId} selected champion {Champion}", playerId, request.Name);
            state.TrySelectChampion(playerId, request.Name);
        }
        return Task.FromResult(new ChampionSelectResponse());
    }

    public override Task<MapVoteResponse> MapVote(MapVoteRequest request, ServerCallContext context)
    {
        if (TryGetPlayerId(context, out var playerId))
        {
            logger.LogInformation("Player {PlayerId} voted for map {MapId}", playerId, request.MapId);
            state.Vote(playerId, request.MapId);
        }
        return Task.FromResult(new MapVoteResponse());
    }

    public override Task<SendChatMessageResponse> SendChatMessage(SendChatMessageRequest request, ServerCallContext context)
    {
        if (!string.IsNullOrWhiteSpace(request.Content) && TryGetPlayerId(context, out var playerId))
        {
            logger.LogDebug("Player {PlayerId} sent chat message", playerId);
            state.SendChat(playerId, request.Content);
        }
        return Task.FromResult(new SendChatMessageResponse());
    }

    private bool TryGetPlayerId(ServerCallContext context, out string playerId)
    {
        var ticket = context.RequestHeaders.FirstOrDefault(h => h.Key == TicketHeader)?.Value;
        if (!string.IsNullOrWhiteSpace(ticket) && ticketStore.TryGetPlayerId(ticket!, out playerId!))
        {
            return true;
        }
        playerId = string.Empty;
        return false;
    }

    private sealed class Observer(Channel<LobbyEvent> channel) : IObserver<LobbyEvent>
    {
        public void OnCompleted() { }
        public void OnError(Exception error) { }
        public void OnNext(LobbyEvent value)
        {
            channel.Writer.TryWrite(value);
        }
    }
}
