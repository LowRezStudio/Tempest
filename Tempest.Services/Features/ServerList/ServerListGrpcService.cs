using Grpc.Core;
using Tempest.Protocol.ServerList;
using ServerListService = Tempest.Protocol.ServerList.ServerList;

namespace Tempest.Services.Features.ServerList;

public class ServerListGrpcService(
    ServerListingRepository repository,
    Tempest.Services.Features.ApiKeys.ApiKeyRepository apiKeyRepository) : ServerListService.ServerListBase
{
    public override Task<CreateLobbyResponse> CreateLobby(CreateLobbyRequest request, ServerCallContext context)
    {
        var apiKey = request.ApiKey;
        if (string.IsNullOrWhiteSpace(apiKey) || !apiKeyRepository.IsKeyValid(apiKey, out var userId, out var userName))
        {
            return Task.FromResult(new CreateLobbyResponse
            {
                Error = new CreateLobbyError
                {
                    Code = CreateLobbyErrorCode.KeyNotAllowed,
                    Message = "Invalid API key"
                }
            });
        }

        if (apiKeyRepository.IsUserBanned(userId!))
        {
            return Task.FromResult(new CreateLobbyResponse
            {
                Error = new CreateLobbyError
                {
                    Code = CreateLobbyErrorCode.KeyNotAllowed,
                    Message = "User is banned"
                }
            });
        }

        if (repository.IsApiKeyInUse(apiKey, TimeSpan.FromMinutes(1)))
        {
            return Task.FromResult(new CreateLobbyResponse
            {
                Error = new CreateLobbyError
                {
                    Code = CreateLobbyErrorCode.KeyNotAllowed,
                    Message = "This API key is already in use by an active server"
                }
            });
        }

        var ticket = Guid.NewGuid().ToString("N");
        var row = new ServerListingRow
        {
            Ip = request.Ip,
            LobbyPort = request.LobbyPort,
            Name = request.Name,
            Gamemode = request.Gamemode,
            Game = string.IsNullOrEmpty(request.Game) ? "Paladins" : request.Game,
            Version = request.Version,
            Tags = request.Tags.ToList(),
            Map = request.Map,
            MapId = request.MapId,
            MaxPlayers = request.MaxPlayers,
            MaxSpectators = request.MaxSpectators,
            JoinInProgress = request.JoinInProgress,
            Joinable = request.Joinable,
            HasPassword = request.HasPassword,
            Country = request.Country,
            Ticket = ticket,
            ApiKey = apiKey,
        };

        var id = repository.Add(row);

        return Task.FromResult(new CreateLobbyResponse
        {
            Success = new CreateLobbySuccess
            {
                Id = id,
                Ticket = ticket
            }
        });
    }

    public override Task<UpdateLobbyResponse> UpdateLobby(UpdateLobbyRequest request, ServerCallContext context)
    {
        var updated = repository.Update(request.Id, request.Ticket, server =>
        {
            if (request.HasPlayers) server = server with { Players = request.Players };
            if (request.HasBots) server = server with { Bots = request.Bots };
            if (request.HasSpectators) server = server with { Spectators = request.Spectators };
            if (request.HasMap) server = server with { Map = request.Map };
            if (request.HasMapId) server = server with { MapId = request.MapId };
            if (request.HasJoinable) server = server with { Joinable = request.Joinable };
            if (request.HasJoinInProgress) server = server with { JoinInProgress = request.JoinInProgress };
            return server;
        });

        if (!updated)
        {
            return Task.FromResult(new UpdateLobbyResponse
            {
                Error = new UpdateLobbyError { Code = UpdateLobbyErrorCode.InvalidTicket }
            });
        }

        return Task.FromResult(new UpdateLobbyResponse { Success = new UpdateLobbySuccess() });
    }

    public override async Task GetServers(GetServersRequest request, IServerStreamWriter<ServerListing> responseStream, ServerCallContext context)
    {
        foreach (var row in repository.GetAll())
        {
            if (!row.Joinable)
            {
                // ponytail: skip non-joinable servers to exclude them from the server browser
                continue;
            }
            await responseStream.WriteAsync(row.ToProto());
        }
    }

    public override Task<GetServerByIdResponse> GetServerById(GetServerByIdRequest request, ServerCallContext context)
    {
        var row = repository.Get(request.Id);
        if (row is null)
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
            Success = row.ToProto()
        });
    }
}
