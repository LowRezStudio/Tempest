using System.Collections.Concurrent;

namespace Tempest.CLI.Server;

internal sealed class InMemoryTicketStore : ITicketStore
{
    private readonly ConcurrentDictionary<string, string> _ticketToPlayer = new();
    private readonly ConcurrentDictionary<string, HashSet<string>> _playerToTickets = new();

    public string Issue(string playerId)
    {
        var ticket = Guid.NewGuid().ToString("N");
        _ticketToPlayer[ticket] = playerId;
        _playerToTickets.AddOrUpdate(playerId,
            _ => new HashSet<string> { ticket },
            (_, set) => { set.Add(ticket); return set; });
        return ticket;
    }

    public bool TryGetPlayerId(string ticket, out string playerId) => _ticketToPlayer.TryGetValue(ticket, out playerId!);

    public void Revoke(string ticket)
    {
        if (_ticketToPlayer.TryRemove(ticket, out var playerId))
        {
            if (_playerToTickets.TryGetValue(playerId, out var set))
            {
                set.Remove(ticket);
                if (set.Count == 0)
                    _playerToTickets.TryRemove(playerId, out _);
            }
        }
    }

    public void RevokeTickets(string playerId)
    {
        if (_playerToTickets.TryRemove(playerId, out var set))
        {
            foreach (var t in set)
            {
                _ticketToPlayer.TryRemove(t, out _);
            }
        }
    }
}
