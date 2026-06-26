using Microsoft.AspNetCore.Server.Kestrel.Core;

namespace Tempest.CLI.Server;

internal sealed class EmbeddedServer
{
    private readonly LobbyServerOptions _options;
    private readonly LobbyState _state;
    private readonly ITicketStore _ticketStore;
    private WebApplication? _app;
    private readonly List<UpnpPortMapper> _upnp = [];

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

        builder.Services.AddCors(options =>
        {
            options.AddPolicy("AllowFrontend",
                policy =>
                {
                    policy
                        .AllowAnyOrigin()
                        .AllowAnyMethod()
                        .AllowAnyHeader()
                        .WithExposedHeaders(
                        "grpc-status",
                        "grpc-message",
                        "grpc-status-details-bin"
                        );
                });
        });
        builder.Services.AddLogging(c => c.ClearProviders());
        builder.Services.AddSingleton(_ticketStore);
        builder.Services.AddSingleton(_state);
        builder.Services.AddSingleton<PlayerDisconnectMonitor>();
        builder.Services.AddHostedService(sp => sp.GetRequiredService<PlayerDisconnectMonitor>());
        builder.Services.AddSingleton<LobbyServiceImpl>();
        builder.Services.AddSingleton(_options);
        builder.Services.AddGrpc();
        if (_options.PublicServer && !string.IsNullOrEmpty(_options.ServicesUrl))
        {
            builder.Services.AddHostedService<ServerListHeartbeat>();
        }

        _app = builder.Build();

        _app.UseCors("AllowFrontend");
        _app.UseGrpcWeb(new GrpcWebOptions { DefaultEnabled = true });
        _app.MapGrpcService<LobbyServiceImpl>();
        _app.MapGet("/health", () => Results.StatusCode(200));

        await _app.StartAsync();

        if (_options.Upnp)
        {
            var ports = new[] { _options.Port, _options.GameServerPort }.Distinct();
            foreach (var port in ports)
            {
                var mapper = new UpnpPortMapper(port, $"Tempest: {_options.Name}");
                try
                {
                    await mapper.MapAsync();
                    Console.WriteLine($"UPnP mapped port {port}");
                    _upnp.Add(mapper);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine($"UPnP mapping failed for port {port}: {ex.Message}");
                    await mapper.DisposeAsync();
                }
            }
        }
    }

    public async Task StopAsync()
    {
        _state.KillGameServer();

        foreach (var mapper in _upnp)
        {
            try
            {
                await mapper.UnmapAsync();
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine($"UPnP unmap failed: {ex.Message}");
            }
            await mapper.DisposeAsync();
        }
        _upnp.Clear();

        if (_app != null)
        {
            await _app.StopAsync(TimeSpan.FromSeconds(2));
            await _app.DisposeAsync();
        }
    }
}
