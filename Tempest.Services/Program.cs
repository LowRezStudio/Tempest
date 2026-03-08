using Microsoft.AspNetCore.Server.Kestrel.Core;
using Tempest.Services;

var builder = WebApplication.CreateBuilder(args);

builder.WebHost.ConfigureKestrel(k =>
{
    k.ListenAnyIP(5197, o => o.Protocols = HttpProtocols.Http1);
    k.ListenAnyIP(5198, o => o.Protocols = HttpProtocols.Http2);
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

builder.Services.AddGrpc();
builder.Services.AddSingleton<InMemoryServerStore>();

var app = builder.Build();

app.UseCors("AllowFrontend");
app.UseGrpcWeb(new GrpcWebOptions { DefaultEnabled = true });
app.MapGrpcService<ServerListServiceImpl>();

app.Run();
