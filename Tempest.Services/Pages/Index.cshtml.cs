using Microsoft.AspNetCore.Mvc.RazorPages;
using Tempest.Services.Features.ServerList;

namespace Tempest.Services.Pages;

public class IndexModel : PageModel
{
    private readonly ServerListingRepository _repository;

    public IndexModel(ServerListingRepository repository)
    {
        _repository = repository;
    }

    public IReadOnlyList<ServerListingRow> Servers { get; private set; } = [];

    public void OnGet()
    {
        Servers = _repository.GetAll()
            .OrderByDescending(s => s.Players)
            .ThenBy(s => s.Name)
            .ToList();
    }
}
