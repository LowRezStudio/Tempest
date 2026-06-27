using Microsoft.Extensions.Logging;
using System.Collections.Concurrent;

namespace Tempest.CLI.Server;

internal sealed class PlayerDisconnectMonitor(LobbyState state, ILogger<PlayerDisconnectMonitor> logger) : BackgroundService
{
    // handling the situation where a player has multiple event streams open for some reason
    private readonly ConcurrentDictionary<string, PlayerConnection> _players = new();
    private readonly TimeSpan _checkInterval = TimeSpan.FromSeconds(60);
    private readonly TimeSpan _maxDisconnectTime = TimeSpan.FromMinutes(5);

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        while (!stoppingToken.IsCancellationRequested)
        {
            var now = DateTime.UtcNow;
            var lobbyPlayers = state.GetInfoEvent().Info.Players;
            var lobbyPlayerIds = new HashSet<string>(lobbyPlayers.Select(p => p.Id));

            //removing player's that have left from dictionary
            foreach (var key in _players.Keys)
            {
                if (!lobbyPlayerIds.Contains(key))
                    _players.TryRemove(key, out _);
            }

            foreach (var player in lobbyPlayers)
            {
                if (_players.TryGetValue(player.Id, out var connection))
                {
                    DateTime? disconnectStart;
                    lock (connection)
                    {
                        disconnectStart = connection.DisconnectStartTime;
                    }
                    if (disconnectStart != null && now - disconnectStart.Value > _maxDisconnectTime)
                    {
                        logger.LogWarning("Player {PlayerId} disconnected for more than {MaxDisconnectTime}, kicking", player.Id, _maxDisconnectTime);
                        state.KickPlayer(player.Id);
                    }
                }
                else // never opened an event stream
                {
                    _players.TryAdd(player.Id, new PlayerConnection { DisconnectStartTime = now });
                }
            }

            await Task.Delay(_checkInterval, stoppingToken);
        }
    }

    public void PlayerConnected(string playerId)
    {
        var connection = _players.GetOrAdd(playerId, _ => new PlayerConnection());
        lock (connection)
        {
            connection.ConnectionCount++;
            connection.DisconnectStartTime = null;
        }
        logger.LogDebug("Player {PlayerId} event stream connected (count: {ConnectionCount})", playerId, connection.ConnectionCount);
    }

    public void PlayerDisconnected(string playerId)
    {
        if (!_players.TryGetValue(playerId, out var connection)) return;
        lock (connection)
        {
            connection.ConnectionCount--;
            if (connection.ConnectionCount <= 0)
                connection.DisconnectStartTime = DateTime.UtcNow;
        }
        logger.LogDebug("Player {PlayerId} event stream disconnected (count: {ConnectionCount})", playerId, connection.ConnectionCount);
    }

    private sealed class PlayerConnection
    {
        public int ConnectionCount;
        public DateTime? DisconnectStartTime;
    }
}
