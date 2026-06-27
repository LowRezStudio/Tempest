using Quartz;
using Tempest.Services;
using Tempest.Services.Jobs;

var builder = WebApplication.CreateSlimBuilder(args);

builder.WebHost.UseKestrelHttpsConfiguration();

builder.Services.AddRouting();
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

builder.Services.AddQuartz(q =>
{
    q.UseInMemoryStore();

    var jobKey = new JobKey(nameof(RemoveDeadServerListingsJob));
    q.AddJob<RemoveDeadServerListingsJob>(jobKey);
    q.AddTrigger(t => t
        .ForJob(jobKey)
        .WithSimpleSchedule(s => s
            .WithInterval(TimeSpan.FromSeconds(30))
            .RepeatForever()));
});
builder.Services.AddQuartzHostedService(q => q.WaitForJobsToComplete = true);

var app = builder.Build();

app.UseCors("AllowFrontend");
app.UseGrpcWeb(new GrpcWebOptions { DefaultEnabled = true });
app.MapGrpcService<ServerListServiceImpl>();

app.Run();
