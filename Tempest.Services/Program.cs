using Microsoft.AspNetCore.Server.Kestrel.Core;
using Tempest.Services;

var builder = WebApplication.CreateBuilder(args);

builder.WebHost.ConfigureKestrel(k =>
{
    // Honor ASPNETCORE_URLS for container deployments; otherwise use the default
    // dual-port setup. 5197 now speaks both HTTP/1.1 and HTTP/2 (h2c) so a single
    // exposed container port works for grpc-web clients and native gRPC callers.
    if (string.IsNullOrEmpty(Environment.GetEnvironmentVariable("ASPNETCORE_URLS")))
    {
        k.ListenAnyIP(5197, o => o.Protocols = HttpProtocols.Http1AndHttp2);
        k.ListenAnyIP(5198, o => o.Protocols = HttpProtocols.Http2);
    }
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
builder.Services.AddHostedService<HeartbeatManager>();
builder.Services.AddSingleton<InMemoryServerStore>();

var app = builder.Build();

app.UseCors("AllowFrontend");
app.UseGrpcWeb(new GrpcWebOptions { DefaultEnabled = true });
app.MapGrpcService<ServerListServiceImpl>();

app.Run();
