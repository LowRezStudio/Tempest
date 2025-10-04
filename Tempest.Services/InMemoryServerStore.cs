using System.Collections.Concurrent;
using Tempest.Protocol.ServerList;

namespace Tempest.Services;

public class InMemoryServerStore
{
    private readonly ConcurrentDictionary<ulong, ServerListing> _servers = new();
    private ulong _nextId = 1;

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
