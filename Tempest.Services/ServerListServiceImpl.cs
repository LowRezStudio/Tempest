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

        _store.Add(server);

        return Task.FromResult(new CreateLobbyResponse
        {
            Success = new CreateLobbySuccess()
        });
    }

    public override async Task GetServers(GetServersRequest request, IServerStreamWriter<ServerListing> responseStream, ServerCallContext context)
    {
        foreach (var server in _store.GetAll())
        {
            await responseStream.WriteAsync(server);
        }
    }

    public override Task<ServerListing> GetServerById(GetServerByIdRequest request, ServerCallContext context)
    {
        if (!ulong.TryParse(request.Id, out var id))
        {
            throw new RpcException(new Status(StatusCode.InvalidArgument, "Invalid server ID"));
        }

        var server = _store.Get(id);
        if (server == null)
        {
            throw new RpcException(new Status(StatusCode.NotFound, "Server not found"));
        }

        return Task.FromResult(server);
    }

    public override Task<HeartbeatLobbyResponse> HeartbeatLobby(HeartbeatLobbyRequest request, ServerCallContext context)
    {
        return Task.FromResult(new HeartbeatLobbyResponse());
    }
}
