using Google.Protobuf.WellKnownTypes;
using Tempest.Protocol.ServerList;

namespace Tempest.Services
{
    public class HeartbeatManager : BackgroundService
    {
        private readonly InMemoryServerStore _store;

        private static readonly HttpClient client = new();
        private static readonly TimeSpan CheckInterval = TimeSpan.FromSeconds(30);
        private static readonly TimeSpan Timeout = TimeSpan.FromSeconds(90);
 
        public HeartbeatManager(InMemoryServerStore store)
        {
            _store = store;
            client.Timeout = TimeSpan.FromSeconds(10);
        }
        protected override async Task ExecuteAsync(CancellationToken stoppingToken)
        {
            while (!stoppingToken.IsCancellationRequested)
            {
                var servers = _store.GetAll();
                var now = Timestamp.FromDateTime(DateTime.UtcNow);
                var requests = servers.Select(async (server) =>
                {
                    //remove when demo entry is no longer needed
                    if (server.Tags.Contains("demo")) return;
                    bool alive = await PingServer(server, stoppingToken);
                    if (alive)
                    {
                        server.LastSeen = now;
                    }
                    else if (server.LastSeen == null || (now - server.LastSeen).ToTimeSpan() > Timeout)
                    {
                        _store.Remove(server.Id);
                    }
                });

                await Task.WhenAll(requests);

                await Task.Delay(CheckInterval, stoppingToken);
            }
        }
        private static async Task<bool> PingServer(ServerListing server, CancellationToken stoppingToken)
        {
            try
            {
                var response = await client.GetAsync($"http://{server.Ip}:{server.LobbyPort}/health", stoppingToken);
                return response.IsSuccessStatusCode;
            }
            catch
            {
                return false;
            }
        }
    }
}
