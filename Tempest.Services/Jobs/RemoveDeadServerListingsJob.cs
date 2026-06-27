using Google.Protobuf.WellKnownTypes;
using Quartz;

namespace Tempest.Services.Jobs;

[DisallowConcurrentExecution]
public class RemoveDeadServerListingsJob : IJob
{
    private readonly InMemoryServerStore _store;
    private readonly ILogger<RemoveDeadServerListingsJob> _logger;

    private static readonly TimeSpan StaleTimeout = TimeSpan.FromMinutes(1);

    public RemoveDeadServerListingsJob(InMemoryServerStore store, ILogger<RemoveDeadServerListingsJob> logger)
    {
        _store = store;
        _logger = logger;
    }

    public Task Execute(IJobExecutionContext context)
    {
        var now = Timestamp.FromDateTime(DateTime.UtcNow);
        var removed = 0;

        foreach (var server in _store.GetAll().ToList())
        {
            if (server.LastSeen == null || (now - server.LastSeen).ToTimeSpan() > StaleTimeout)
            {
                if (_store.Remove(server.Id))
                {
                    removed++;
                }
            }
        }

        if (removed > 0)
        {
            _logger.LogInformation("Removed {RemovedCount} dead server listing(s).", removed);
        }

        return Task.CompletedTask;
    }
}
