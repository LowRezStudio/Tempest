using System.Diagnostics.CodeAnalysis;
using System.Net;
using System.Net.Sockets;
using Microsoft.AspNetCore.Server.Kestrel.Core;

namespace Tempest.CLI.Server;

internal sealed class EmbeddedServer
{
    private readonly LobbyServerOptions _options;
    private readonly LobbyState _state;
    private readonly ITicketStore _ticketStore;
    private WebApplication? _app;
    private ServerListClient? _serverListClient;

    public EmbeddedServer(LobbyServerOptions options)
    {
        _options = options;
        _ticketStore = new InMemoryTicketStore();
        _state = new LobbyState(options, _ticketStore);
    }
    
    public async Task StartAsync()
    {
        var builder = WebApplication.CreateSlimBuilder(new WebApplicationOptions());
        builder.WebHost.ConfigureKestrel(k =>
        {
            k.ListenAnyIP(_options.Port, o =>
            {
                o.Protocols = HttpProtocols.Http1AndHttp2;
            });
        });

        builder.Services.AddLogging(c => c.ClearProviders());
        builder.Services.AddSingleton<ITicketStore>(_ticketStore);
        builder.Services.AddSingleton(_state);
        builder.Services.AddSingleton<LobbyServiceImpl>();
        builder.Services.AddGrpc();

        _app = builder.Build();

        _app.MapGrpcService<LobbyServiceImpl>();
        _app.MapGet("/health", () => Results.Ok(new { status = "ok" }));

        await _app.StartAsync();

        if (_options.PublicServer && !string.IsNullOrEmpty(_options.ServicesUrl))
        {
            await RegisterWithServerListAsync();
        }
    }

    private async Task RegisterWithServerListAsync()
    {
        try
        {
            _serverListClient = new ServerListClient(_options.ServicesUrl!);
            
            var localIp = GetLocalIpAddress();
            var request = new Protocol.ServerList.CreateLobbyRequest
            {
                Ip = localIp,
                LobbyPort = (uint)_options.Port,
                Name = _options.Name,
                Game = "Paladins",
                Version = _options.Version,
                MaxPlayers = (uint)_options.MaxPlayers,
                MaxSpectators = 0,
                JoinInProgress = _options.JoinInProgress,
                Joinable = true,
                HasPassword = !string.IsNullOrEmpty(_options.Password),
                Country = Protocol.Common.CountryCode.Us
            };

            if (!string.IsNullOrEmpty(_options.Map))
                request.Map = _options.Map;

            foreach (var tag in _options.Tags)
            {
                request.Tags.Add(tag);
            }

            var response = await _serverListClient.CreateLobbyAsync(request);
            
            if (response.ResultCase == Protocol.ServerList.CreateLobbyResponse.ResultOneofCase.Success)
            {
                Console.WriteLine("Successfully registered with ServerList service");
            }
            else if (response.ResultCase == Protocol.ServerList.CreateLobbyResponse.ResultOneofCase.Error)
            {
                Console.WriteLine($"Failed to register with ServerList: {response.Error.Message}");
            }
        }
        catch (Exception ex)
        {
            Console.WriteLine($"Failed to register with ServerList: {ex.Message}");
        }
    }

    private static string GetLocalIpAddress()
    {
        using var socket = new Socket(AddressFamily.InterNetwork, SocketType.Dgram, 0);
        socket.Connect("8.8.8.8", 65530);
        var endPoint = socket.LocalEndPoint as IPEndPoint;
        return endPoint?.Address.ToString() ?? "127.0.0.1";
    }

    public async Task StopAsync()
    {
        if (_app != null)
        {
            await _app.StopAsync();
            await _app.DisposeAsync();
        }
        
        _serverListClient?.Dispose();
    }
}
