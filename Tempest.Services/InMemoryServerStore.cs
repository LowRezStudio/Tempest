using System.Collections.Concurrent;
using Tempest.Protocol.ServerList;

namespace Tempest.Services;

public class InMemoryServerStore
{
    private readonly ConcurrentDictionary<string, ServerListing> _servers = new();

    public string Add(ServerListing server)
    {
        var id = Guid.NewGuid().ToString();
        server.Id = id;
        _servers[id] = server;
        return id;
    }

    public bool Update(string id, ServerListing server)
    {
        if (!_servers.ContainsKey(id))
            return false;
        
        server.Id = id;
        _servers[id] = server;
        return true;
    }

    public bool Remove(string id)
    {
        return _servers.TryRemove(id, out _);
    }

    public ServerListing? Get(string id)
    {
        _servers.TryGetValue(id, out var server);
        return server;
    }

    public IEnumerable<ServerListing> GetAll()
    {
        return _servers.Values;
    }
}
