using AspNet.Security.OAuth.GitHub;
using Microsoft.AspNetCore.HttpOverrides;
using Quartz;
using Tempest.Services;
using Tempest.Services.Endpoints;
using Tempest.Services.Features.ServerList;
using Tempest.Services.Jobs;
using Tempest.Services.Persistence;

var builder = WebApplication.CreateSlimBuilder(args);

builder.WebHost.UseKestrelHttpsConfiguration();

// Routing / CORS
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

// Forwarded Headers (for reverse proxy behind TLS)
builder.Services.Configure<ForwardedHeadersOptions>(options =>
{
    options.ForwardedHeaders = ForwardedHeaders.XForwardedFor | ForwardedHeaders.XForwardedProto;
    options.KnownIPNetworks.Clear();
    options.KnownProxies.Clear();
    options.ForwardLimit = null;
});

// gRPC + Razor Pages
builder.Services.AddGrpc();
builder.Services.AddRazorPages();

// Minimal API VSA — auto-registered IEndpoint implementations
builder.Services.AddEndpoints();

// Persistence — ADO.NET + Dapper over SQLite
builder.Services.AddScoped<SqliteConnectionFactory>();
builder.Services.AddHostedService<DatabaseInitializer>();

// Features
builder.Services.AddScoped<ServerListingRepository>();
builder.Services.AddScoped<Tempest.Services.Features.ApiKeys.ApiKeyRepository>();

// Authentication
var githubClientId = builder.Configuration["Authentication:GitHub:ClientId"];
var githubClientSecret = builder.Configuration["Authentication:GitHub:ClientSecret"];
var githubConfigured = !string.IsNullOrWhiteSpace(githubClientId) && !string.IsNullOrWhiteSpace(githubClientSecret);

var authenticationBuilder = builder.Services
    .AddAuthentication(options =>
    {
        options.DefaultScheme = "Cookies";
        options.DefaultChallengeScheme = githubConfigured
            ? GitHubAuthenticationDefaults.AuthenticationScheme
            : "Cookies";
    })
    .AddCookie(options =>
    {
        options.LoginPath = "/Login";
        options.LogoutPath = "/Logout";
        options.AccessDeniedPath = "/Login";
        options.Cookie.SecurePolicy = CookieSecurePolicy.Always; // ponytail: force HTTPS for session cookies
    });

if (githubConfigured)
{
    authenticationBuilder.AddGitHub(options =>
    {
        options.ClientId = githubClientId!;
        options.ClientSecret = githubClientSecret!;
        options.Scope.Add("read:user");
        options.CorrelationCookie.SecurePolicy = CookieSecurePolicy.Always; // ponytail: force HTTPS for OAuth correlation cookie
    });
}

builder.Services.AddAuthorization(options =>
{
    options.AddPolicy("Authenticated", policy => policy.RequireAuthenticatedUser());
});

// Background jobs
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

app.UseForwardedHeaders();

if (!githubConfigured)
{
    app.Logger.LogWarning("GitHub OAuth credentials are not configured (Authentication:GitHub:ClientId/ClientSecret). Login will be unavailable until they are set.");
}

app.UseCors("AllowFrontend");
app.UseGrpcWeb(new GrpcWebOptions { DefaultEnabled = true });

app.UseStaticFiles();
app.UseAuthentication();
app.UseAuthorization();

app.MapGrpcService<ServerListGrpcService>();
app.MapRazorPages();
app.MapEndpoints();

app.Run();
