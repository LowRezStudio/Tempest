using Microsoft.Data.Sqlite;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;

namespace Tempest.Services.Persistence;

/// <summary>
/// Runs lightweight idempotent schema migrations on startup. Each migration is a single
/// SQL statement guarded by <c>IF NOT EXISTS</c> so re-running is safe. Migrations are
/// tracked in a <c>__migrations</c> table for forward compatibility.
/// </summary>
public sealed class DatabaseInitializer(IServiceProvider services) : IHostedLifecycleService
{
    private static readonly (string Id, string Sql)[] Migrations =
    [
        ("0001_server_listings", """
            CREATE TABLE IF NOT EXISTS server_listings (
                id              TEXT PRIMARY KEY,
                ticket          TEXT NOT NULL,
                ip              TEXT NOT NULL,
                lobby_port      INTEGER NOT NULL,
                name            TEXT NOT NULL,
                game            TEXT NOT NULL,
                version         TEXT NOT NULL,
                tags            TEXT NOT NULL DEFAULT '[]',
                map             TEXT,
                map_id          TEXT,
                players         INTEGER NOT NULL DEFAULT 0,
                max_players     INTEGER NOT NULL DEFAULT 0,
                bots            INTEGER NOT NULL DEFAULT 0,
                max_spectators  INTEGER NOT NULL DEFAULT 0,
                spectators      INTEGER NOT NULL DEFAULT 0,
                join_in_progress INTEGER NOT NULL DEFAULT 0,
                joinable        INTEGER NOT NULL DEFAULT 0,
                has_password    INTEGER NOT NULL DEFAULT 0,
                country         INTEGER NOT NULL DEFAULT 0,
                auth_methods    TEXT NOT NULL DEFAULT '[]',
                last_seen       TEXT NOT NULL
            );
            """),
        ("0002_api_keys_and_bans", """
            CREATE TABLE IF NOT EXISTS api_keys (
                key          TEXT PRIMARY KEY,
                user_id      TEXT NOT NULL,
                user_name    TEXT NOT NULL,
                created_at   TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS banned_users (
                user_id      TEXT PRIMARY KEY,
                banned_at    TEXT NOT NULL,
                reason       TEXT
            );
            ALTER TABLE server_listings ADD COLUMN api_key TEXT;
            """),
    ];

    public async Task StartAsync(CancellationToken cancellationToken)
    {
        using var scope = services.CreateScope();
        var factory = scope.ServiceProvider.GetRequiredService<SqliteConnectionFactory>();
        var connection = factory.Create();

        await EnsureMigrationsTableAsync(connection, cancellationToken);
        await RunMigrationsAsync(connection, cancellationToken);
    }

    public Task StopAsync(CancellationToken cancellationToken) => Task.CompletedTask;

    public Task StartingAsync(CancellationToken cancellationToken) => Task.CompletedTask;
    public Task StartedAsync(CancellationToken cancellationToken) => Task.CompletedTask;
    public Task StoppingAsync(CancellationToken cancellationToken) => Task.CompletedTask;
    public Task StoppedAsync(CancellationToken cancellationToken) => Task.CompletedTask;

    private static async Task EnsureMigrationsTableAsync(SqliteConnection connection, CancellationToken ct)
    {
        await using var cmd = connection.CreateCommand();
        cmd.CommandText = """
            CREATE TABLE IF NOT EXISTS __migrations (
                id         TEXT PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
            """;
        await cmd.ExecuteNonQueryAsync(ct);
    }

    private static async Task RunMigrationsAsync(SqliteConnection connection, CancellationToken ct)
    {
        foreach (var (id, sql) in Migrations)
        {
            await using var check = connection.CreateCommand();
            check.CommandText = "SELECT COUNT(*) FROM __migrations WHERE id = $id;";
            check.Parameters.AddWithValue("$id", id);

            var applied = Convert.ToInt32(await check.ExecuteScalarAsync(ct));
            if (applied > 0)
            {
                continue;
            }

            await using var apply = connection.CreateCommand();
            apply.CommandText = sql;
            await apply.ExecuteNonQueryAsync(ct);

            await using var record = connection.CreateCommand();
            record.CommandText = "INSERT INTO __migrations (id, applied_at) VALUES ($id, $at);";
            record.Parameters.AddWithValue("$id", id);
            record.Parameters.AddWithValue("$at", DateTimeOffset.UtcNow.ToString("O"));
            await record.ExecuteNonQueryAsync(ct);
        }
    }
}
