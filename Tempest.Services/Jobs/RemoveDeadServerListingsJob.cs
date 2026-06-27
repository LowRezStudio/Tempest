using Quartz;
using Tempest.Services.Features.ServerList;

namespace Tempest.Services.Jobs;

[DisallowConcurrentExecution]
public class RemoveDeadServerListingsJob : IJob
{
    private readonly ServerListingRepository _repository;
    private readonly ILogger<RemoveDeadServerListingsJob> _logger;

    private static readonly TimeSpan StaleTimeout = TimeSpan.FromMinutes(1);

    public RemoveDeadServerListingsJob(ServerListingRepository repository, ILogger<RemoveDeadServerListingsJob> logger)
    {
        _repository = repository;
        _logger = logger;
    }

    public Task Execute(IJobExecutionContext context)
    {
        var stale = _repository.GetStale(StaleTimeout);
        var removed = 0;

        foreach (var server in stale)
        {
            if (_repository.Remove(server.Id))
            {
                removed++;
            }
        }

        if (removed > 0)
        {
            _logger.LogInformation("Removed {RemovedCount} dead server listing(s).", removed);
        }

        return Task.CompletedTask;
    }
}
