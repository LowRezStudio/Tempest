using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.Json.Serialization.Metadata;

namespace Tempest.CLI.Mods;

/// <summary>
/// Client for the mod repository (mods.lowrezstudio.com, contract/openapi.yaml in that repo).
/// Reads: anonymous. Writes: an API key from <c>tempest mod login</c>, stored in
/// <c>&lt;app data&gt;/com.lowrezstudio.tempest/repo.json</c>; TEMPEST_REPO_URL and
/// TEMPEST_REPO_KEY override both.
/// </summary>
public sealed class ModRepoClient : IDisposable
{
    public const string DefaultUrl = "https://mods.lowrezstudio.com";

    private readonly HttpClient _http;
    public Uri BaseUri { get; }
    public string? ApiKey { get; }

    public ModRepoClient(string? url = null, string? apiKey = null)
    {
        var config = RepoConfig.Load();
        var envUrl = Environment.GetEnvironmentVariable("TEMPEST_REPO_URL");
        var baseUrl = url ?? (string.IsNullOrEmpty(envUrl) ? config.Url ?? DefaultUrl : envUrl);
        BaseUri = new Uri(baseUrl.TrimEnd('/') + "/");
        var envKey = Environment.GetEnvironmentVariable("TEMPEST_REPO_KEY");
        ApiKey = apiKey ?? (string.IsNullOrEmpty(envKey) ? config.ApiKey : envKey);
        _http = new HttpClient { Timeout = TimeSpan.FromMinutes(30) };
        _http.DefaultRequestHeaders.UserAgent.ParseAdd("tempest-cli");
        if (ApiKey != null) _http.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", ApiKey);
    }

    private Uri Api(string path) => new(BaseUri, "api/v1/" + path.TrimStart('/'));

    private async Task<T> ReadAsync<T>(HttpResponseMessage res, JsonTypeInfo<T> type)
    {
        if (!res.IsSuccessStatusCode)
        {
            RepoError? err = null;
            try { err = await res.Content.ReadFromJsonAsync(RepoJsonContext.Default.RepoError); } catch (Exception) { /* not json */ }
            throw new RepoException(err?.Error ?? ((int)res.StatusCode).ToString(), err?.Message ?? $"{(int)res.StatusCode} {res.ReasonPhrase}");
        }
        return await res.Content.ReadFromJsonAsync(type) ?? throw new RepoException("empty", "empty response");
    }

    public async Task<RepoModPage> SearchAsync(string? q, string? patch, string? kind, int page = 1, int perPage = 20)
    {
        var query = new List<string> { $"page={page}", $"per_page={perPage}" };
        if (!string.IsNullOrWhiteSpace(q)) query.Add($"q={Uri.EscapeDataString(q)}");
        if (!string.IsNullOrWhiteSpace(patch)) query.Add($"patch={Uri.EscapeDataString(patch)}");
        if (!string.IsNullOrWhiteSpace(kind)) query.Add($"kind={Uri.EscapeDataString(kind)}");
        using var res = await _http.GetAsync(Api("mods?" + string.Join('&', query)));
        return await ReadAsync(res, RepoJsonContext.Default.RepoModPage);
    }

    public async Task<RepoMod?> GetModAsync(string id)
    {
        using var res = await _http.GetAsync(Api($"mods/{Uri.EscapeDataString(id)}"));
        if (res.StatusCode == System.Net.HttpStatusCode.NotFound) return null;
        return await ReadAsync(res, RepoJsonContext.Default.RepoMod);
    }

    public async Task<List<RepoVersion>> GetVersionsAsync(string id)
    {
        using var res = await _http.GetAsync(Api($"mods/{Uri.EscapeDataString(id)}/versions"));
        if (res.StatusCode == System.Net.HttpStatusCode.NotFound) throw new RepoException("not_found", $"{id} is not on {BaseUri.Host}");
        return await ReadAsync(res, RepoJsonContext.Default.ListRepoVersion);
    }

    /// <summary>Downloads a version through the counting endpoint to <paramref name="destination"/>.</summary>
    public async Task DownloadAsync(string id, string version, string destination)
    {
        using var res = await _http.GetAsync(Api($"mods/{Uri.EscapeDataString(id)}/versions/{Uri.EscapeDataString(version)}/download"), HttpCompletionOption.ResponseHeadersRead);
        if (!res.IsSuccessStatusCode) throw new RepoException(((int)res.StatusCode).ToString(), $"download failed: {(int)res.StatusCode} {res.ReasonPhrase}");
        await using var file = File.Create(destination);
        await res.Content.CopyToAsync(file);
    }

    public async Task<RepoMe> MeAsync()
    {
        using var res = await _http.GetAsync(Api("me"));
        return await ReadAsync(res, RepoJsonContext.Default.RepoMe);
    }

    /// <summary>Uploads a package: intent, presigned PUT, complete, then polls until inspected.</summary>
    public async Task<RepoIntent> PublishAsync(string package, RepoListing listing, Action<string>? progress = null)
    {
        if (ApiKey == null) throw new RepoException("unauthorized", "no API key; run `tempest mod login <key>` (create one on the website under Settings)");
        var info = new FileInfo(package);
        string sha;
        await using (var stream = File.OpenRead(package))
            sha = Convert.ToHexStringLower(await SHA256.HashDataAsync(stream));

        progress?.Invoke("requesting upload");
        using var intentRes = await _http.PostAsJsonAsync(Api("publish/intents"), new RepoIntentRequest { Size = info.Length, Sha256 = sha, Filename = info.Name }, RepoJsonContext.Default.RepoIntentRequest);
        var created = await ReadAsync(intentRes, RepoJsonContext.Default.RepoIntentCreated);

        progress?.Invoke($"uploading {info.Length / 1024.0 / 1024.0:F1} MB");
        await using (var stream = File.OpenRead(package))
        {
            using var put = new HttpRequestMessage(HttpMethod.Put, created.UploadUrl) { Content = new StreamContent(stream) };
            put.Content.Headers.ContentType = new MediaTypeHeaderValue("application/zip");
            put.Content.Headers.ContentLength = info.Length;
            using var bare = new HttpClient { Timeout = TimeSpan.FromHours(2) };
            using var putRes = await bare.SendAsync(put);
            if (!putRes.IsSuccessStatusCode) throw new RepoException("upload_failed", $"upload failed: {(int)putRes.StatusCode} {putRes.ReasonPhrase}");
        }

        progress?.Invoke("inspecting");
        using var completeRes = await _http.PostAsJsonAsync(Api($"publish/intents/{created.IntentId}/complete"), listing, RepoJsonContext.Default.RepoListing);
        await ReadAsync(completeRes, RepoJsonContext.Default.RepoIntentState);

        for (var i = 0; i < 400; i++)
        {
            await Task.Delay(1500);
            using var res = await _http.GetAsync(Api($"publish/intents/{created.IntentId}"));
            var state = await ReadAsync(res, RepoJsonContext.Default.RepoIntent);
            if (state.State is "done" or "failed") return state;
        }
        throw new RepoException("timeout", "inspection did not finish in time; check the dashboard");
    }

    public void Dispose() => _http.Dispose();
}

public sealed class RepoException(string code, string message) : Exception(message)
{
    public string Code { get; } = code;
}

/// <summary><c>repo.json</c> next to the signing key: the repository URL and the API key.</summary>
public sealed class RepoConfig
{
    public string? Url { get; set; }
    public string? ApiKey { get; set; }

    public static string Path =>
        System.IO.Path.GetFullPath(System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "com.lowrezstudio.tempest", "repo.json"));

    public static RepoConfig Load()
    {
        try
        {
            return File.Exists(Path) ? JsonSerializer.Deserialize(File.ReadAllText(Path), RepoJsonContext.Default.RepoConfig) ?? new() : new();
        }
        catch (Exception)
        {
            return new();
        }
    }

    public void Save()
    {
        Directory.CreateDirectory(System.IO.Path.GetDirectoryName(Path)!);
        File.WriteAllText(Path, JsonSerializer.Serialize(this, RepoJsonContext.Default.RepoConfig));
    }
}

// ---- wire types (camelCase, see contract/openapi.yaml) -------------------------------------

public sealed class RepoError { public string? Error { get; set; } public string? Message { get; set; } }

public sealed class RepoModPage { public List<RepoMod> Items { get; set; } = []; public int Total { get; set; } public int Page { get; set; } public int PerPage { get; set; } }

public sealed class RepoOwner { public string Handle { get; set; } = ""; public string DisplayName { get; set; } = ""; public bool Team { get; set; } }

public sealed class RepoMod
{
    public string Id { get; set; } = "";
    public string Name { get; set; } = "";
    public string Summary { get; set; } = "";
    public string Kind { get; set; } = "";
    public List<string> Tags { get; set; } = [];
    public RepoOwner Owner { get; set; } = new();
    public long Downloads { get; set; }
    public string UpdatedAt { get; set; } = "";
    public RepoVersion? Latest { get; set; }
    public string Url { get; set; } = "";
}

public sealed class RepoSigner { public string Fingerprint { get; set; } = ""; public string Name { get; set; } = ""; public string Trust { get; set; } = ""; }

public sealed class RepoCompatRange
{
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public string? From { get; set; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public string? To { get; set; }
}

public sealed class RepoCompat { public List<string> Patches { get; set; } = []; public List<RepoCompatRange> Ranges { get; set; } = []; }

public sealed class RepoVersion
{
    public string Id { get; set; } = "";
    public string ModId { get; set; } = "";
    public string Version { get; set; } = "";
    public string Status { get; set; } = "";
    public string Changelog { get; set; } = "";
    public long Size { get; set; }
    public string Sha256 { get; set; } = "";
    public string DownloadUrl { get; set; } = "";
    public RepoSigner? Signer { get; set; }
    public RepoCompat Compat { get; set; } = new();
    public long Downloads { get; set; }
}

public sealed class RepoMe { public string Handle { get; set; } = ""; public string DisplayName { get; set; } = ""; public string Role { get; set; } = ""; public bool Team { get; set; } }

public sealed class RepoIntentRequest { public long Size { get; set; } public string Sha256 { get; set; } = ""; public string Filename { get; set; } = ""; }

public sealed class RepoIntentCreated { public string IntentId { get; set; } = ""; public string UploadUrl { get; set; } = ""; public string ExpiresAt { get; set; } = ""; }

public sealed class RepoIntentState { public string IntentId { get; set; } = ""; public string State { get; set; } = ""; }

public sealed class RepoListing
{
    public RepoCompat Compat { get; set; } = new();
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public string? Changelog { get; set; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public string? Summary { get; set; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public List<string>? Tags { get; set; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public string? Repository { get; set; }
}

public sealed class RepoIntentResult
{
    public string? ModId { get; set; }
    public string? Version { get; set; }
    public List<string> Errors { get; set; } = [];
    public List<string> Warnings { get; set; } = [];
}

public sealed class RepoIntent
{
    public string IntentId { get; set; } = "";
    public string State { get; set; } = "";
    public string? VersionId { get; set; }
    public RepoIntentResult? Result { get; set; }
}

[JsonSourceGenerationOptions(PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase, PropertyNameCaseInsensitive = true)]
[JsonSerializable(typeof(RepoError))]
[JsonSerializable(typeof(RepoModPage))]
[JsonSerializable(typeof(RepoMod))]
[JsonSerializable(typeof(List<RepoVersion>))]
[JsonSerializable(typeof(RepoVersion))]
[JsonSerializable(typeof(RepoMe))]
[JsonSerializable(typeof(RepoIntentRequest))]
[JsonSerializable(typeof(RepoIntentCreated))]
[JsonSerializable(typeof(RepoIntentState))]
[JsonSerializable(typeof(RepoListing))]
[JsonSerializable(typeof(RepoIntent))]
[JsonSerializable(typeof(RepoConfig))]
internal partial class RepoJsonContext : JsonSerializerContext
{
}
