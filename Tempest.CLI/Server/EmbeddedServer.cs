using Microsoft.AspNetCore.Server.Kestrel.Core;
using ZLogger;

namespace Tempest.CLI.Server;

internal sealed class EmbeddedServer
{
    private readonly LobbyServerOptions _options;
    private readonly LobbyState _state;
    private readonly ITicketStore _ticketStore;
    private readonly ILogger<EmbeddedServer> _logger;
    private WebApplication? _app;
    private readonly List<UpnpPortMapper> _upnp = [];

    public EmbeddedServer(LobbyServerOptions options, ILoggerFactory loggerFactory)
    {
        _options = options;
        _logger = loggerFactory.CreateLogger<EmbeddedServer>();
        _ticketStore = new InMemoryTicketStore();
        _state = new LobbyState(options, _ticketStore, loggerFactory.CreateLogger<LobbyState>());
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
        builder.Logging.ClearProviders();
        builder.Logging.AddZLoggerConsole();
        builder.Services.AddSingleton(_ticketStore);
        builder.Services.AddSingleton(_state);
        builder.Services.AddSingleton<PlayerDisconnectMonitor>();
        builder.Services.AddHostedService(sp => sp.GetRequiredService<PlayerDisconnectMonitor>());
        builder.Services.AddSingleton<LobbyServiceImpl>();
        builder.Services.AddSingleton(_options);
        builder.Services.AddGrpc();
        if (_options.Discover)
        {
            builder.Services.AddHostedService<LanDiscoveryResponder>();
        }
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
        _logger.LogInformation("Embedded server started on port {Port}", _options.Port);

        try
        {
            await FirewallHelper.OpenPortsAsync(_options.Port, _options.GameServerPort, _logger);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to open firewall ports");
        }

        if (_options.Upnp)
        {
            var ports = new[] { _options.Port, _options.GameServerPort }.Distinct();
            foreach (var port in ports)
            {
                var mapper = new UpnpPortMapper(port, $"Tempest: {_options.Name}");
                try
                {
                    await mapper.MapAsync();
                    _logger.LogInformation("UPnP mapped port {Port}", port);
                    _upnp.Add(mapper);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "UPnP mapping failed for port {Port}", port);
                    await mapper.DisposeAsync();
                }
            }
        }
    }

    public async Task StopAsync()
    {
        _logger.LogInformation("Embedded server stopping");
        _state.KillGameServer();

        try
        {
            await FirewallHelper.ClosePortsAsync(_options.Port, _options.GameServerPort, _logger);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Failed to close firewall ports");
        }

        foreach (var mapper in _upnp)
        {
            try
            {
                await mapper.UnmapAsync();
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "UPnP unmap failed");
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
