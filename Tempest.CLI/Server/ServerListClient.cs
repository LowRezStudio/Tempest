using Grpc.Net.Client;
using Tempest.Protocol.ServerList;

namespace Tempest.CLI.Server;

internal sealed class ServerListClient : IDisposable
{
    private readonly GrpcChannel _channel;
    private readonly ServerList.ServerListClient _client;

    public ServerListClient(string serverUrl)
    {
        _channel = GrpcChannel.ForAddress(serverUrl);
        _client = new ServerList.ServerListClient(_channel);
    }

    public async Task<CreateLobbyResponse> CreateLobbyAsync(CreateLobbyRequest request, CancellationToken cancellationToken = default)
    {
        return await _client.CreateLobbyAsync(request, cancellationToken: cancellationToken);
    }

    public async Task<List<ServerListing>> GetServersAsync(CancellationToken cancellationToken = default)
    {
        var request = new GetServersRequest();
        var call = _client.GetServers(request, cancellationToken: cancellationToken);
        var servers = new List<ServerListing>();

        while (await call.ResponseStream.MoveNext(cancellationToken))
        {
            servers.Add(call.ResponseStream.Current);
        }

        return servers;
    }

    public async Task<ServerListing> GetServerByIdAsync(string id, CancellationToken cancellationToken = default)
    {
        var request = new GetServerByIdRequest { Id = id };
        return await _client.GetServerByIdAsync(request, cancellationToken: cancellationToken);
    }

    public async Task<HeartbeatLobbyResponse> HeartbeatLobbyAsync(CancellationToken cancellationToken = default)
    {
        var request = new HeartbeatLobbyRequest();
        return await _client.HeartbeatLobbyAsync(request, cancellationToken: cancellationToken);
    }

    public void Dispose()
    {
        _channel.Dispose();
    }
}
