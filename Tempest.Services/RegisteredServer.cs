using Tempest.Protocol.ServerList;

namespace Tempest.Services;

internal sealed record RegisteredServer(ServerListing Listing, string Ticket);
