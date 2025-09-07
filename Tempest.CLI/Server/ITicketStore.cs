namespace Tempest.CLI.Server;

internal interface ITicketStore
{
    string Issue(string playerId);
    bool TryGetPlayerId(string ticket, out string playerId);
    void Revoke(string ticket);
    void RevokeTickets(string playerId);
}
