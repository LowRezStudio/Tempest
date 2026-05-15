using Grpc.Core;
using Tempest.Protocol.ServerList;

namespace Tempest.Services;

public class ServerListServiceImpl : ServerList.ServerListBase
{
    private readonly InMemoryServerStore _store;

    public ServerListServiceImpl(InMemoryServerStore store)
    {
        _store = store;
    }

    public override Task<CreateLobbyResponse> CreateLobby(CreateLobbyRequest request, ServerCallContext context)
    {
        var server = new ServerListing
        {
            Ip = request.Ip,
            LobbyPort = request.LobbyPort,
            Name = request.Name,
            Game = request.Game,
            Version = request.Version,
            Tags = { request.Tags },
            Map = request.Map,
            MapId = request.MapId,
            MaxPlayers = request.MaxPlayers,
            MaxSpectators = request.MaxSpectators,
            JoinInProgress = request.JoinInProgress,
            Joinable = request.Joinable,
            HasPassword = request.HasPassword,
            Country = request.Country,
            Players = 0,
            Bots = 0,
            Spectators = 0
        };

        var id = _store.Add(server);

        return Task.FromResult(new CreateLobbyResponse
        {
            Success = new CreateLobbySuccess
            {
                Id = id
            }
        });
    }

    public override Task<UpdateLobbyResponse> UpdateLobby(UpdateLobbyRequest request, ServerCallContext context)
    {
        var server = _store.Get(request.Id);
        if (server == null)
        {
            return Task.FromResult(new UpdateLobbyResponse
            {
                Error = new UpdateLobbyError { Code = UpdateLobbyErrorCode.NotFound }
            });
        }

        if (request.HasPlayers) server.Players = request.Players;
        if (request.HasBots) server.Bots = request.Bots;
        if (request.HasSpectators) server.Spectators = request.Spectators;
        if (request.HasMap) server.Map = request.Map;
        if (request.HasMapId) server.MapId = request.MapId;
        if (request.HasJoinable) server.Joinable = request.Joinable;
        if (request.HasJoinInProgress) server.JoinInProgress = request.JoinInProgress;

        return Task.FromResult(new UpdateLobbyResponse { Success = new UpdateLobbySuccess() });
    }

    public override async Task GetServers(GetServersRequest request, IServerStreamWriter<ServerListing> responseStream, ServerCallContext context)
    {
        foreach (var server in _store.GetAll())
        {
            await responseStream.WriteAsync(server);
        }
    }

    public override Task<GetServerByIdResponse> GetServerById(GetServerByIdRequest request, ServerCallContext context)
    {
        var server = _store.Get(request.Id);
        if (server == null)
        {
            return Task.FromResult(new GetServerByIdResponse
            {
                Error = new GetServerByIdError
                {
                    Code = GetServerByIdErrorCode.IdNotFound,
                    Message = "Server not found"
                }
            });
        }

        return Task.FromResult(new GetServerByIdResponse
        {
            Success = server
        });
    }

    public override Task<HeartbeatLobbyResponse> HeartbeatLobby(HeartbeatLobbyRequest request, ServerCallContext context)
    {
        return Task.FromResult(new HeartbeatLobbyResponse());
    }
}
