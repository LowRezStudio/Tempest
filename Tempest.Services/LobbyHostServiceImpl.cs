using System.Security.Cryptography;
using System.Text;
using Grpc.Core;
using Tempest.Protocol;

namespace Tempest.Services;

public class LobbyHostServiceImpl(HostService hostService) : LobbyHostService.LobbyHostServiceBase
{
    public override async Task CreateLobby(IAsyncStreamReader<LobbyRequest> requestStream, IServerStreamWriter<LobbyUpdate> responseStream, ServerCallContext context)
    {
        LobbyHost? lobbyHost = null;
        
        await foreach (var request in requestStream.ReadAllAsync())
        {
            switch (request.MessageCase)
            {
                case LobbyRequest.MessageOneofCase.Init:
                {
                    var init = request.Init;
                    var publicId = BitConverter.ToUInt64(SHA256.HashData(Encoding.UTF8.GetBytes(init.PrivateId)));

                    init.Server.Id = publicId;

                    lobbyHost = new LobbyHost()
                    {
                        Stream = responseStream,
                        Lobby = new Lobby()
                        {
                            Server = init.Server
                        }
                    };

                    lock (hostService.List)
                    {
                        hostService.List.Add(lobbyHost);
                    }

                    await responseStream.WriteAsync(new LobbyUpdate()
                    {
                        MessageId = (ulong)Random.Shared.NextInt64(),
                        Init = new InitLobbyUpdate()
                        {
                            Id = publicId
                        }
                    });
                    
                    break;
                }
            }
        }
        
        if (lobbyHost != null) lock (hostService.List)
        {
            hostService.List.Remove(lobbyHost);
        }
    }
}