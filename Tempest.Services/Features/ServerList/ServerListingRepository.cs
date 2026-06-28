using Dapper;
using Microsoft.Data.Sqlite;
using Tempest.Services.Persistence;

namespace Tempest.Services.Features.ServerList;

/// <summary>
/// Dapper-backed repository for <see cref="ServerListingRow"/>. Registered as a scoped
/// service so it shares a connection with the request's <see cref="SqliteConnectionFactory"/>.
/// </summary>
public sealed class ServerListingRepository(SqliteConnectionFactory connectionFactory)
{
    private SqliteConnection Connection => connectionFactory.Create();

    public string Add(ServerListingRow row)
    {
        var id = string.IsNullOrEmpty(row.Id) ? Guid.NewGuid().ToString() : row.Id;
        var now = DateTimeOffset.UtcNow.ToString("O");

        Connection.Execute("""
            INSERT INTO server_listings (
                id, ticket, ip, lobby_port, name, game, version, tags, map, map_id,
                players, max_players, bots, max_spectators, spectators,
                join_in_progress, joinable, has_password, country, auth_methods, last_seen
            ) VALUES (
                $id, $ticket, $ip, $lobbyPort, $name, $game, $version, $tags, $map, $mapId,
                $players, $maxPlayers, $bots, $maxSpectators, $spectators,
                $joinInProgress, $joinable, $hasPassword, $country, $authMethods, $lastSeen
            );
            """, new
        {
            id,
            ticket = row.Ticket,
            ip = row.Ip,
            lobbyPort = (long)row.LobbyPort,
            name = row.Name,
            game = row.Game,
            version = row.Version,
            tags = row.TagsJson,
            map = row.Map,
            mapId = row.MapId,
            players = (long)row.Players,
            maxPlayers = (long)row.MaxPlayers,
            bots = (long)row.Bots,
            maxSpectators = (long)row.MaxSpectators,
            spectators = (long)row.Spectators,
            joinInProgress = row.JoinInProgress ? 1L : 0L,
            joinable = row.Joinable ? 1L : 0L,
            hasPassword = row.HasPassword ? 1L : 0L,
            country = (long)row.Country,
            authMethods = row.AuthMethodsJson,
            lastSeen = now,
        });

        return id;
    }

    public bool Update(string id, string ticket, Func<ServerListingRow, ServerListingRow> update)
    {
        var existing = Get(id);
        if (existing is null || existing.Ticket != ticket)
        {
            return false;
        }

        var updated = update(existing with { LastSeen = DateTimeOffset.UtcNow });

        Connection.Execute("""
            UPDATE server_listings SET
                players = $players,
                bots = $bots,
                spectators = $spectators,
                map = $map,
                map_id = $mapId,
                joinable = $joinable,
                join_in_progress = $joinInProgress,
                last_seen = $lastSeen
            WHERE id = $id AND ticket = $ticket;
            """, new
        {
            players = (long)updated.Players,
            bots = (long)updated.Bots,
            spectators = (long)updated.Spectators,
            map = updated.Map,
            mapId = updated.MapId,
            joinable = updated.Joinable ? 1L : 0L,
            joinInProgress = updated.JoinInProgress ? 1L : 0L,
            lastSeen = updated.LastSeen.ToString("O"),
            id,
            ticket,
        });

        return true;
    }

    public bool Remove(string id) =>
        Connection.Execute("DELETE FROM server_listings WHERE id = $id;", new { id }) > 0;

    public ServerListingRow? Get(string id)
    {
        var raw = Connection.QuerySingleOrDefault<RawRow>(
            SelectSql + " WHERE id = $id;", new { id });

        return raw is null ? null : raw.ToRow();
    }

    public IReadOnlyList<ServerListingRow> GetAll() =>
        Connection.Query<RawRow>(SelectSql).Select(r => r.ToRow()).AsList();

    public IReadOnlyList<ServerListingRow> GetStale(TimeSpan timeout)
    {
        var cutoff = DateTimeOffset.UtcNow.Subtract(timeout).ToString("O");
        return Connection.Query<RawRow>(SelectSql + " WHERE last_seen < $cutoff;", new { cutoff })
            .Select(r => r.ToRow())
            .AsList();
    }

    private const string SelectSql = """
        SELECT
            id              AS Id,
            ticket          AS Ticket,
            ip              AS Ip,
            lobby_port      AS LobbyPort,
            name            AS Name,
            game            AS Game,
            version         AS Version,
            tags            AS TagsJson,
            map             AS Map,
            map_id          AS MapId,
            players         AS Players,
            max_players     AS MaxPlayers,
            bots            AS Bots,
            max_spectators  AS MaxSpectators,
            spectators      AS Spectators,
            join_in_progress AS JoinInProgress,
            joinable        AS Joinable,
            has_password    AS HasPassword,
            country         AS Country,
            auth_methods    AS AuthMethodsJson,
            last_seen       AS LastSeen
        FROM server_listings
        """;

    /// <summary>
    /// Raw Dapper shape — JSON columns stay as strings and are deserialized by
    /// <see cref="ServerListingRow.FromReader"/> when promoted to a typed row.
    /// </summary>
    private sealed class RawRow
    {
        public string Id { get; init; } = string.Empty;
        public string Ticket { get; init; } = string.Empty;
        public string Ip { get; init; } = string.Empty;
        public long LobbyPort { get; init; }
        public string Name { get; init; } = string.Empty;
        public string Game { get; init; } = string.Empty;
        public string Version { get; init; } = string.Empty;
        public string TagsJson { get; init; } = "[]";
        public string? Map { get; init; }
        public string? MapId { get; init; }
        public long Players { get; init; }
        public long MaxPlayers { get; init; }
        public long Bots { get; init; }
        public long MaxSpectators { get; init; }
        public long Spectators { get; init; }
        public long JoinInProgress { get; init; }
        public long Joinable { get; init; }
        public long HasPassword { get; init; }
        public long Country { get; init; }
        public string AuthMethodsJson { get; init; } = "[]";
        public string LastSeen { get; init; } = string.Empty;

        public ServerListingRow ToRow() => ServerListingRow.FromReader(
            Id, Ticket, Ip, LobbyPort, Name, Game, Version, TagsJson, Map, MapId,
            Players, MaxPlayers, Bots, MaxSpectators, Spectators,
            JoinInProgress, Joinable, HasPassword, Country, AuthMethodsJson, LastSeen);
    }
}
