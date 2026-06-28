using Microsoft.AspNetCore.Mvc;
using Microsoft.AspNetCore.Mvc.RazorPages;
using Tempest.Services.Features.ApiKeys;
using Tempest.Services.Features.ServerList;

namespace Tempest.Services.Pages;

public class IndexModel : PageModel
{
    private readonly ServerListingRepository _repository;
    private readonly ApiKeyRepository _apiKeyRepository;

    public IndexModel(ServerListingRepository repository, ApiKeyRepository apiKeyRepository)
    {
        _repository = repository;
        _apiKeyRepository = apiKeyRepository;
    }

    public IReadOnlyList<ServerListingRow> Servers { get; private set; } = [];
    public IReadOnlyList<ApiKeyRow> ApiKeys { get; private set; } = [];
    public bool IsBanned { get; private set; }

    public void OnGet()
    {
        Servers = _repository.GetAll()
            .OrderByDescending(s => s.Players)
            .ThenBy(s => s.Name)
            .ToList();

        if (User.Identity?.IsAuthenticated == true)
        {
            var userId = User.FindFirst(System.Security.Claims.ClaimTypes.NameIdentifier)?.Value;
            if (userId is not null)
            {
                ApiKeys = _apiKeyRepository.GetKeysForUser(userId);
                IsBanned = _apiKeyRepository.IsUserBanned(userId);
            }
        }
    }

    public IActionResult OnPostCreateKey()
    {
        if (User.Identity?.IsAuthenticated != true)
        {
            return Challenge();
        }

        var userId = User.FindFirst(System.Security.Claims.ClaimTypes.NameIdentifier)?.Value;
        var userName = User.Identity.Name ?? "GitHub User";
        if (userId is not null)
        {
            if (_apiKeyRepository.IsUserBanned(userId))
            {
                TempData["Error"] = "You are banned and cannot create API keys.";
                return RedirectToPage();
            }

            var key = $"tempest_sk_{Guid.NewGuid():N}";
            if (_apiKeyRepository.CreateKeyForUser(userId, userName, key))
            {
                TempData["Message"] = "Successfully created a new API key!";
            }
            else
            {
                TempData["Error"] = "Failed to create API key. Limit of 5 keys reached.";
            }
        }
        return RedirectToPage();
    }

    public IActionResult OnPostDeleteKey(string key)
    {
        if (User.Identity?.IsAuthenticated != true)
        {
            return Challenge();
        }

        var userId = User.FindFirst(System.Security.Claims.ClaimTypes.NameIdentifier)?.Value;
        if (userId is not null && !string.IsNullOrWhiteSpace(key))
        {
            if (_apiKeyRepository.DeleteKey(key, userId))
            {
                TempData["Message"] = "API key deleted.";
            }
        }
        return RedirectToPage();
    }
}
