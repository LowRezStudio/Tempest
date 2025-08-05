using ConsoleAppFramework;
using Grpc.Core;
using Grpc.Net.Client;
using Tempest.Protocol;

namespace Tempest.CLI;

internal class ServerCommands
{
    public async Task Open(
        [Argument] string path,
        string name = "Paladins Server",
        string tags = "",
        string map = "",
        string version = "0.57",
        uint maxPlayers = 10,
        bool joinInProgress = false,
        string? gamemode = null,
        string? publicIp = null,
        string? privateId = null,
        string servicesUrl = "https://localhost:7165")
    {
        using var channel = GrpcChannel.ForAddress(servicesUrl);
        var client = new LobbyHostService.LobbyHostServiceClient(channel);

        Console.WriteLine($"Services URL: {servicesUrl}");

        var stream = client.CreateLobby();

        await stream.RequestStream.WriteAsync(new LobbyRequest()
        {
            MessageId = (ulong)Random.Shared.NextInt64(),
            Init = new InitLobbyRequest()
            {
                PrivateId = privateId ?? Guid.NewGuid().ToString(),
                Server = new Server()
                {
                    Bots = 0,
                    Game = "Paladins",
                    Joinable = true,
                    JoinInProgress = joinInProgress,
                    Map = map,
                    MaxPlayers = maxPlayers,
                    Name = name,
                    Players = 0,
                    Tags = tags,
                    Version = version
                }
            }
        });

        _ = Task.Run(() =>
        {
            // command prompt here
        });

        await foreach (var update in stream.ResponseStream.ReadAllAsync())
        {
            switch (update.MessageCase)
            {
                case LobbyUpdate.MessageOneofCase.Init:
                {
                    Console.WriteLine($"Server Init, public id: {update.Init.Id}");
                    
                    break;
                }
                default:
                {
                    Console.WriteLine($"Unhandled event: {update.MessageCase.ToString()}");

                    break;
                }
            }
        }
    }
}