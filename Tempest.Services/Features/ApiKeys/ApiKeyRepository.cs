using Dapper;
using Microsoft.Data.Sqlite;
using Tempest.Services.Persistence;

namespace Tempest.Services.Features.ApiKeys;

public sealed record ApiKeyRow(string Key, string UserId, string UserName, DateTimeOffset CreatedAt);

public sealed class ApiKeyRepository(SqliteConnectionFactory connectionFactory)
{
    private SqliteConnection Connection => connectionFactory.Create();

    public IReadOnlyList<ApiKeyRow> GetKeysForUser(string userId)
    {
        return Connection.Query<RawRow>("""
            SELECT key AS Key, user_id AS UserId, user_name AS UserName, created_at AS CreatedAt
            FROM api_keys
            WHERE user_id = $userId
            ORDER BY created_at DESC;
            """, new { userId })
            .Select(r => r.ToRow())
            .ToList();
    }

    public int GetKeyCountForUser(string userId)
    {
        return Connection.ExecuteScalar<int>("""
            SELECT COUNT(*) FROM api_keys WHERE user_id = $userId;
            """, new { userId });
    }

    public bool CreateKeyForUser(string userId, string userName, string key)
    {
        if (GetKeyCountForUser(userId) >= 5)
        {
            return false;
        }

        Connection.Execute("""
            INSERT INTO api_keys (key, user_id, user_name, created_at)
            VALUES ($key, $userId, $userName, $createdAt);
            """, new
        {
            key,
            userId,
            userName,
            createdAt = DateTimeOffset.UtcNow.ToString("O")
        });
        return true;
    }

    public bool DeleteKey(string key, string userId)
    {
        return Connection.Execute("""
            DELETE FROM api_keys WHERE key = $key AND user_id = $userId;
            """, new { key, userId }) > 0;
    }

    public bool IsKeyValid(string key, out string? userId, out string? userName)
    {
        var row = Connection.QuerySingleOrDefault<RawRow>("""
            SELECT key AS Key, user_id AS UserId, user_name AS UserName, created_at AS CreatedAt
            FROM api_keys
            WHERE key = $key;
            """, new { key });

        if (row is null)
        {
            userId = null;
            userName = null;
            return false;
        }

        userId = row.UserId;
        userName = row.UserName;
        return true;
    }

    public bool IsUserBanned(string userId)
    {
        return Connection.ExecuteScalar<int>("""
            SELECT COUNT(*) FROM banned_users WHERE user_id = $userId;
            """, new { userId }) > 0;
    }

    public void BanUser(string userId, string? reason = null)
    {
        Connection.Execute("""
            INSERT OR REPLACE INTO banned_users (user_id, banned_at, reason)
            VALUES ($userId, $bannedAt, $reason);
            """, new
        {
            userId,
            bannedAt = DateTimeOffset.UtcNow.ToString("O"),
            reason
        });
    }

    public void UnbanUser(string userId)
    {
        Connection.Execute("""
            DELETE FROM banned_users WHERE user_id = $userId;
            """, new { userId });
    }

    private sealed class RawRow
    {
        public string Key { get; init; } = string.Empty;
        public string UserId { get; init; } = string.Empty;
        public string UserName { get; init; } = string.Empty;
        public string CreatedAt { get; init; } = string.Empty;

        public ApiKeyRow ToRow() => new(
            Key,
            UserId,
            UserName,
            DateTimeOffset.Parse(CreatedAt, null, System.Globalization.DateTimeStyles.RoundtripKind)
        );
    }
}
