using System.Diagnostics;
using Tempest.Services.Features.ApiKeys;
using Tempest.Services.Features.ServerList;
using Tempest.Services.Persistence;

namespace Tempest.Services.Endpoints;

public class SelfCheckEndpoint : IEndpoint
{
    public static void Map(IEndpointRouteBuilder app)
    {
        app.MapGet("/api/self-check", (
            ApiKeyRepository apiKeys,
            ServerListingRepository servers,
            SqliteConnectionFactory connectionFactory) =>
        {
            try
            {
                RunCheck(apiKeys, servers);
                return Results.Ok(new { status = "OK", message = "All checks passed successfully." });
            }
            catch (Exception ex)
            {
                return Results.Problem(detail: ex.ToString(), title: "Self check failed");
            }
        });
    }

    private static void RunCheck(ApiKeyRepository apiKeys, ServerListingRepository servers)
    {
        var testUserId = "test-user-" + Guid.NewGuid().ToString("N");
        var testUserName = "TestyMcTestFace";

        // 1. Initial count is 0
        if (apiKeys.GetKeyCountForUser(testUserId) != 0)
            throw new Exception("Initial key count should be 0");

        // 2. Create up to 5 keys
        var keys = new List<string>();
        for (int i = 0; i < 5; i++)
        {
            var key = $"tempest_sk_test_{i}_{Guid.NewGuid():N}";
            var created = apiKeys.CreateKeyForUser(testUserId, testUserName, key);
            if (!created)
                throw new Exception($"Should succeed creating key {i}");
            keys.Add(key);
        }

        // 3. Count should now be 5
        if (apiKeys.GetKeyCountForUser(testUserId) != 5)
            throw new Exception("Key count should be 5");

        // 4. Creating a 6th key must fail (up to 5 keys allowed)
        var extraKey = $"tempest_sk_test_6_{Guid.NewGuid():N}";
        var createdExtra = apiKeys.CreateKeyForUser(testUserId, testUserName, extraKey);
        if (createdExtra)
            throw new Exception("Should fail creating a 6th key");

        // 5. Check if keys are valid
        foreach (var key in keys)
        {
            var isValid = apiKeys.IsKeyValid(key, out var uid, out var uname);
            if (!isValid)
                throw new Exception("Key should be valid");
            if (uid != testUserId)
                throw new Exception("User ID should match");
            if (uname != testUserName)
                throw new Exception("User name should match");
        }

        // 6. Test ban validation
        if (apiKeys.IsUserBanned(testUserId))
            throw new Exception("User should not be banned initially");
        apiKeys.BanUser(testUserId, "Test ban");
        if (!apiKeys.IsUserBanned(testUserId))
            throw new Exception("User should be banned after BanUser");

        apiKeys.UnbanUser(testUserId);
        if (apiKeys.IsUserBanned(testUserId))
            throw new Exception("User should be unbanned after UnbanUser");

        // 7. Revoke/delete key
        var deleted = apiKeys.DeleteKey(keys[0], testUserId);
        if (!deleted)
            throw new Exception("Should succeed in deleting the first key");
        if (apiKeys.GetKeyCountForUser(testUserId) != 4)
            throw new Exception("Key count should be 4 after deletion");

        var isValidDeleted = apiKeys.IsKeyValid(keys[0], out _, out _);
        if (isValidDeleted)
            throw new Exception("Deleted key must no longer be valid");
    }
}
