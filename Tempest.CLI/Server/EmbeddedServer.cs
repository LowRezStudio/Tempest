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
    }

    public async Task StopAsync()
    {
        if (_app != null)
        {
            await _app.StopAsync();
            await _app.DisposeAsync();
        }
    }
}
