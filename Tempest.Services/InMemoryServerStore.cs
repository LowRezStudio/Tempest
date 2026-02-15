using System.Collections.Concurrent;
using Tempest.Protocol.Common;
using Tempest.Protocol.ServerList;

namespace Tempest.Services;

public class InMemoryServerStore
{
    private readonly ConcurrentDictionary<ulong, ServerListing> _servers = new();
    private ulong _nextId = 1;
    private readonly ulong _dummyId;

    public InMemoryServerStore()
    {
        var server = new ServerListing
        {
            Ip = "127.0.0.1",
            LobbyPort = 7777,
            Name = "Tempest Demo Server",
            Game = "Tempest",
            Version = "0.0.0",
            Tags = { "demo", "local" },
            Map = "Training Grounds",
            MapId = "0",
            MaxPlayers = 10,
            MaxSpectators = 2,
            JoinInProgress = true,
            Joinable = true,
            HasPassword = false,
            Country = CountryCode.Us,
            Players = 0,
            Bots = 0,
            Spectators = 0
        };

        _dummyId = Add(server);
    }

    public ulong Add(ServerListing server)
    {
        var id = Interlocked.Increment(ref _nextId) - 1;
        server.Id = id;
        _servers[id] = server;
        return id;
    }

    public bool Update(ulong id, ServerListing server)
    {
        if (!_servers.ContainsKey(id))
            return false;
        
        server.Id = id;
        _servers[id] = server;
        return true;
    }

    public bool Remove(ulong id)
    {
        if (id == _dummyId)
            return false;

        return _servers.TryRemove(id, out _);
    }

    public ServerListing? Get(ulong id)
    {
        _servers.TryGetValue(id, out var server);
        return server;
    }

    public IEnumerable<ServerListing> GetAll()
    {
        return _servers.Values;
    }
}
