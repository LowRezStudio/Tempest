using System.Collections.Concurrent;
using Google.Protobuf.WellKnownTypes;
using Tempest.Protocol.ServerList;

namespace Tempest.Services;

public class InMemoryServerStore
{
    private readonly ConcurrentDictionary<string, RegisteredServer> _servers = new();

    public string Add(ServerListing server, string ticket)
    {
        var id = Guid.NewGuid().ToString();
        server.Id = id;
        server.LastSeen = Timestamp.FromDateTime(DateTime.UtcNow);
        _servers[id] = new RegisteredServer(server, ticket);
        return id;
    }

    public bool Update(string id, string ticket, Action<ServerListing> update)
    {
        if (!_servers.TryGetValue(id, out var registered))
            return false;

        if (registered.Ticket != ticket)
            return false;

        update(registered.Listing);
        registered.Listing.LastSeen = Timestamp.FromDateTime(DateTime.UtcNow);
        return true;
    }

    public bool Remove(string id)
    {
        return _servers.TryRemove(id, out _);
    }

    public ServerListing? Get(string id)
    {
        _servers.TryGetValue(id, out var registered);
        return registered?.Listing;
    }

    public IEnumerable<ServerListing> GetAll()
    {
        return _servers.Values.Select(r => r.Listing);
    }
}
