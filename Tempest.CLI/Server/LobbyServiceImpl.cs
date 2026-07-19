using Grpc.Core;
using System.Collections.Concurrent;
using System.Threading.Channels;
using Tempest.Protocol.Common;
using Tempest.Protocol.Lobby;

namespace Tempest.CLI.Server;

internal sealed class LobbyServiceImpl(LobbyState state, ITicketStore ticketStore, PlayerDisconnectMonitor playerDisconnectMonitor, LobbyServerOptions options, IPBanStore banStore, ILogger<LobbyServiceImpl> logger) : Lobby.LobbyBase
{
    private const string TicketHeader = "x-ticket";
    private readonly ConcurrentDictionary<string, string> _playerIPs = new(); // playerId → IP

    public override Task<JoinLobbyResponse> JoinLobby(JoinLobbyRequest request, ServerCallContext context)
    {
        var clientIP = GetClientIP(context);
        logger.LogInformation("JoinLobby request from {AuthValue} using {AuthMethod} (IP: {IP})", request.AuthValue, request.AuthMethod, clientIP);

        // Check IP ban
        if (banStore.IsBanned(clientIP))
        {
            var ban = banStore.GetBan(clientIP);
            logger.LogWarning("JoinLobby rejected for banned IP {IP}: {Reason}", clientIP, ban?.Reason);
            return Task.FromResult(new JoinLobbyResponse
            {
                Error = new JoinLobbyError
                {
                    Code = JoinLobbyErrorCode.LobbyInvalid,
                    Message = $"You are banned: {ban?.Reason ?? "No reason provided"}"
                }
            });
        }

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
            _playerIPs[playerId] = clientIP;
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
            Gamemode = options.GameMode ?? string.Empty,
            Game = "Paladins",
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
        var subscription = state.Subscribe(request.PlayerId).Subscribe(new Observer(channel));
        logger.LogInformation("Player {PlayerId} connected to event stream", request.PlayerId);
        playerDisconnectMonitor.PlayerConnected(request.PlayerId);
        try
        {
            //writing the initial data so the client is up to date
            await responseStream.WriteAsync(state.GetInfoEvent(), context.CancellationToken);

            // If player was already kicked before opening the stream, close immediately
            if (state.IsKicked(request.PlayerId))
            {
                logger.LogInformation("Player {PlayerId} was already kicked, closing stream", request.PlayerId);
                return;
            }

            while (await channel.Reader.WaitToReadAsync(context.CancellationToken))
            {
                while (channel.Reader.TryRead(out var evt))
                {
                    await responseStream.WriteAsync(evt, context.CancellationToken);

                    // If this event is a kick for this player, close the stream
                    if (evt.PlayerKicked != null && evt.PlayerKicked.PlayerId == request.PlayerId)
                    {
                        logger.LogInformation("Player {PlayerId} was kicked, closing event stream", request.PlayerId);
                        return;
                    }
                }
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
            state.ClearKicked(request.PlayerId);
            _playerIPs.TryRemove(request.PlayerId, out _);
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
        if (string.IsNullOrWhiteSpace(request.Content))
        {
            logger.LogWarning("SendChatMessage failed: empty message from IP {IP}", GetClientIP(context));
            return Task.FromResult(new SendChatMessageResponse
            {
                Error = new SendChatMessageError
                {
                    Code = SendChatMessageErrorCode.EmptyMessage,
                    Message = "Message is empty"
                }
            });
        }

        if (!TryGetPlayerId(context, out var playerId))
        {
            return Task.FromResult(new SendChatMessageResponse
            {
                Error = new SendChatMessageError
                {
                    Code = SendChatMessageErrorCode.Unspecified,
                    Message = "Not authenticated"
                }
            });
        }

        if (request.Channel is not "global" and not "team")
        {
            return Task.FromResult(new SendChatMessageResponse
            {
                Error = new SendChatMessageError
                {
                    Code = SendChatMessageErrorCode.InvalidChannel,
                    Message = "Invalid channel"
                }
            });
        }

        logger.LogDebug("Player {PlayerId} sent chat message in channel {Channel}", playerId, request.Channel);
        state.SendChat(playerId, request.Content, request.Channel);
        return Task.FromResult(new SendChatMessageResponse
        {
            Success = new SendChatMessageSuccess()
        });
    }

    public override Task<SendCommandResponse> SendCommand(SendCommandRequest request, ServerCallContext context)
    {
        if (!TryGetPlayerId(context, out var playerId))
        {
            return Task.FromResult(new SendCommandResponse
            {
                Error = new SendCommandError
                {
                    Code = SendCommandErrorCode.Forbidden,
                    Message = "Not authenticated"
                }
            });
        }

        var command = request.Command.ToLowerInvariant();
        var args = request.Arguments;
        logger.LogInformation("Player {PlayerId} sent command: {Command} {Args}", playerId, command, args);

        switch (command)
        {
            case "ready":
                return Task.FromResult(new SendCommandResponse
                {
                    Success = new SendCommandSuccess { Message = "You are marked as ready" }
                });

            default:
                return Task.FromResult(new SendCommandResponse
                {
                    Error = new SendCommandError
                    {
                        Code = SendCommandErrorCode.UnknownCommand,
                        Message = $"Unknown command: {command}"
                    }
                });
        }
    }

    public bool TryGetPlayerIP(string playerId, out string ip) => _playerIPs.TryGetValue(playerId, out ip!);

    public string? GetPlayerIP(string playerId) => _playerIPs.TryGetValue(playerId, out var ip) ? ip : null;

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

    private static string GetClientIP(ServerCallContext context)
    {
        var peer = context.Peer;
        if (!string.IsNullOrWhiteSpace(peer))
        {
            // Strip port if present (format is "ipv4:ip:port" or "ipv6:[ip]:port")
            var lastColon = peer.LastIndexOf(':');
            return lastColon > 0 ? peer[..lastColon] : peer;
        }

        return "unknown";
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
