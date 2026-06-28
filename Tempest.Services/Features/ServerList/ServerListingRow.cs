using System.Text.Json;
using Tempest.Protocol.Common;
using Tempest.Protocol.ServerList;

namespace Tempest.Services.Features.ServerList;

/// <summary>
/// Database-backed representation of a registered server listing.
/// Stored as a single row in <c>server_listings</c>; <see cref="Tags"/> and
/// <see cref="AuthMethods"/> are serialized as JSON arrays.
/// </summary>
public sealed record ServerListingRow
{
    public string Id { get; init; } = string.Empty;
    public string Ticket { get; init; } = string.Empty;
    public string Ip { get; init; } = string.Empty;
    public uint LobbyPort { get; init; }
    public string Name { get; init; } = string.Empty;
    public string Game { get; init; } = string.Empty;
    public string Version { get; init; } = string.Empty;
    public IReadOnlyList<string> Tags { get; init; } = [];
    public string? Map { get; init; }
    public string? MapId { get; init; }
    public uint Players { get; init; }
    public uint MaxPlayers { get; init; }
    public uint Bots { get; init; }
    public uint MaxSpectators { get; init; }
    public uint Spectators { get; init; }
    public bool JoinInProgress { get; init; }
    public bool Joinable { get; init; }
    public bool HasPassword { get; init; }
    public CountryCode Country { get; init; }
    public IReadOnlyList<AuthMethod> AuthMethods { get; init; } = [];
    public DateTimeOffset LastSeen { get; init; }
    public string? ApiKey { get; init; }

    /// <summary>
    /// Maps this row to the gRPC <see cref="ServerListing"/> message.
    /// </summary>
    public ServerListing ToProto() => new()
    {
        Id = Id,
        Ip = Ip,
        LobbyPort = LobbyPort,
        Name = Name,
        Game = Game,
        Version = Version,
        Tags = { Tags },
        Map = Map ?? string.Empty,
        MapId = MapId ?? string.Empty,
        Players = Players,
        MaxPlayers = MaxPlayers,
        Bots = Bots,
        MaxSpectators = MaxSpectators,
        Spectators = Spectators,
        JoinInProgress = JoinInProgress,
        Joinable = Joinable,
        HasPassword = HasPassword,
        Country = Country,
        LastSeen = Google.Protobuf.WellKnownTypes.Timestamp.FromDateTimeOffset(LastSeen),
        AuthMethods = { AuthMethods },
    };

    /// <summary>
    /// Hydrates a row from a Dapper-read dictionary, parsing the JSON columns.
    /// </summary>
    internal static ServerListingRow FromReader(
        string id, string ticket, string ip, long lobbyPort, string name, string game, string version,
        string tagsJson, string? map, string? mapId,
        long players, long maxPlayers, long bots, long maxSpectators, long spectators,
        long joinInProgress, long joinable, long hasPassword, long country,
        string authMethodsJson, string lastSeen, string? apiKey)
    {
        return new ServerListingRow
        {
            Id = id,
            Ticket = ticket,
            Ip = ip,
            LobbyPort = (uint)lobbyPort,
            Name = name,
            Game = game,
            Version = version,
            Tags = DeserializeStrings(tagsJson),
            Map = map,
            MapId = mapId,
            Players = (uint)players,
            MaxPlayers = (uint)maxPlayers,
            Bots = (uint)bots,
            MaxSpectators = (uint)maxSpectators,
            Spectators = (uint)spectators,
            JoinInProgress = joinInProgress != 0,
            Joinable = joinable != 0,
            HasPassword = hasPassword != 0,
            Country = (CountryCode)country,
            AuthMethods = DeserializeAuthMethods(authMethodsJson),
            LastSeen = DateTimeOffset.Parse(lastSeen, null, System.Globalization.DateTimeStyles.RoundtripKind),
            ApiKey = apiKey,
        };
    }

    internal string TagsJson => JsonSerializer.Serialize(Tags);
    internal string AuthMethodsJson => JsonSerializer.Serialize(AuthMethods.Select(a => (int)a));

    private static IReadOnlyList<string> DeserializeStrings(string json) =>
        string.IsNullOrWhiteSpace(json) || json == "[]"
            ? []
            : JsonSerializer.Deserialize<List<string>>(json) ?? [];

    private static IReadOnlyList<AuthMethod> DeserializeAuthMethods(string json)
    {
        if (string.IsNullOrWhiteSpace(json) || json == "[]")
        {
            return [];
        }

        var ints = JsonSerializer.Deserialize<List<int>>(json) ?? [];
        return ints.Select(i => (AuthMethod)i).ToList();
    }
}
