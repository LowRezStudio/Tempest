using Grpc.Core;
using Tempest.Protocol;

namespace Tempest.Services;

public class LobbyHost
{
    public required IServerStreamWriter<LobbyUpdate> Stream { get; set; }
    public required Lobby Lobby { get; set; }
}