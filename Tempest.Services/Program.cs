using Microsoft.AspNetCore.Server.Kestrel.Core;
using Tempest.Services;

var builder = WebApplication.CreateBuilder(args);

builder.WebHost.ConfigureKestrel(options =>
{
    options.ConfigureEndpointDefaults(endpointOptions =>
    {
        endpointOptions.Protocols = HttpProtocols.Http1AndHttp2;
    });
});

builder.Services.AddGrpc();
builder.Services.AddSingleton<InMemoryServerStore>();

var app = builder.Build();

app.UseGrpcWeb(new GrpcWebOptions { DefaultEnabled = true });
app.MapGrpcService<ServerListServiceImpl>();

app.Run();