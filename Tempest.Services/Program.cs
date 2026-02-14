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
app.MapGrpcService<ServerListServiceImpl>().EnableGrpcWeb();

app.Run();