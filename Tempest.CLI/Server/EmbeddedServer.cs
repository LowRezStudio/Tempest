using System.Diagnostics.CodeAnalysis;
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
    
    public Task StartAsync()
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

        return _app.StartAsync();
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
