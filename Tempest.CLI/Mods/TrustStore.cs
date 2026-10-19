using System.Text;
using Org.BouncyCastle.Bcpg.OpenPgp;

namespace Tempest.CLI.Mods;

/// <summary>
/// The keys mod signatures are checked against, from the repo's <c>keys/</c> lists: <c>trusted.keys</c>
/// (armored public key blocks) and <c>untrusted.keys</c> (fingerprints or long key IDs). Both are
/// compiled in as a fallback and published to <see cref="DefaultListUrl"/> with a signature by one of
/// <see cref="ListSigners"/>, so a key can be trusted or revoked without a release.
/// </summary>
/// <remarks>
/// With the published lists, trusted = built-in + published trusted.keys + <c>*.pub</c> in the game's
/// and the global keys folder. When they cannot be fetched or verified, only the built-in keys are
/// trusted. Revocations always win: the built-in list, the published one or, offline, the last
/// published one this machine verified.
/// </remarks>
public sealed class TrustStore
{
    public const string DefaultListUrl = "https://cdn.lowrezstudio.com/keys/";
    public const string TrustedListName = "trusted.keys";
    public const string UntrustedListName = "untrusted.keys";

    /// <summary>Keys allowed to sign the published lists (Tom Ferriere, Kyiro); they must be in the built-in trusted.keys.</summary>
    private static readonly string[] ListSigners =
    [
        "6CBE4141BC87826813E27D88BFBA572CC7464148",
        "BB863D614D81B8E7C875717FABA2CFFCB2928EEC",
    ];

    private static readonly SemaphoreSlim Gate = new(1, 1);
    private static PublishedLists? _published;

    private readonly List<PgpPublicKeyRingBundle> _trusted;
    private readonly List<string> _revoked;

    /// <summary>Why the published lists are not in use; null when they are.</summary>
    public string? OfflineReason { get; }

    private TrustStore(List<PgpPublicKeyRingBundle> trusted, List<string> revoked, string? offlineReason)
    {
        _trusted = trusted;
        _revoked = revoked;
        OfflineReason = offlineReason;
    }

    /// <param name="resolvedGame">Adds that game's <c>.tempest/keys</c> folder to the trusted sources.</param>
    public static async Task<TrustStore> LoadAsync(string? resolvedGame)
    {
        var published = await GetPublishedAsync();

        var trusted = ParseKeyList(ReadBuiltIn(TrustedListName));
        var revoked = ParseRevocations(ReadBuiltIn(UntrustedListName));
        if (published.Untrusted != null)
            revoked.AddRange(ParseRevocations(published.Untrusted));

        if (published.Trusted != null)
        {
            trusted.AddRange(ParseKeyList(published.Trusted));

            var dirs = new List<string> { TempestPathUtility.GetGlobalKeysDirectory() };
            if (resolvedGame != null)
                dirs.Insert(0, TempestPathUtility.GetLocalKeysDirectory(resolvedGame));
            foreach (var file in dirs.Where(Directory.Exists).SelectMany(d => Directory.GetFiles(d, "*.pub")))
                trusted.AddRange(ParseKeyList(await File.ReadAllTextAsync(file)));
        }

        return new TrustStore(trusted, revoked, published.OfflineReason);
    }

    /// <summary>Primary keys of every trusted source, revoked ones included.</summary>
    public IEnumerable<PgpPublicKey> TrustedKeys =>
        _trusted.SelectMany(bundle => bundle.GetKeyRings()).Select(ring => ring.GetPublicKey());

    public IReadOnlyList<string> RevokedEntries => _revoked;

    public static string ListSourceDescription => ListSource();

    public PgpPublicKey? FindTrustedKey(long keyId) =>
        _trusted.Select(bundle => bundle.GetPublicKey(keyId)).FirstOrDefault(key => key != null);

    /// <param name="fingerprintOrKeyId">A 40-digit fingerprint or a 16-digit long key ID.</param>
    public bool IsRevoked(string fingerprintOrKeyId) =>
        _revoked.Any(r => fingerprintOrKeyId.EndsWith(r, StringComparison.OrdinalIgnoreCase) ||
                          r.EndsWith(fingerprintOrKeyId, StringComparison.OrdinalIgnoreCase));

    /// <summary>Signs a list file for publishing, writing <c>&lt;file&gt;.asc</c>.</summary>
    public static void SignList(string path, PgpSecretKey key)
    {
        if (!ListSigners.Contains(ModSigning.Fingerprint(key.PublicKey), StringComparer.OrdinalIgnoreCase))
            throw new Exception($"{ModSigning.Describe(key.PublicKey)} is not allowed to sign key lists.");
        File.WriteAllText(path + ".asc", ModSigning.SignText(File.ReadAllText(path), key));
    }

    /// <summary>
    /// Fetched once per process. The untrusted list is cached after every successful fetch so its
    /// revocations still apply offline; trust is never taken from the cache.
    /// </summary>
    private static async Task<PublishedLists> GetPublishedAsync()
    {
        await Gate.WaitAsync();
        try
        {
            if (_published != null) return _published;

            var cacheDir = Path.Combine(Path.GetDirectoryName(TempestPathUtility.GetGlobalKeysDirectory())!, "keys-cache");
            try
            {
                var (trusted, untrusted, untrustedAsc) = await FetchAsync();
                Directory.CreateDirectory(cacheDir);
                await File.WriteAllTextAsync(Path.Combine(cacheDir, UntrustedListName), untrusted);
                await File.WriteAllTextAsync(Path.Combine(cacheDir, UntrustedListName + ".asc"), untrustedAsc);
                _published = new PublishedLists(trusted, untrusted, null);
            }
            catch (Exception ex)
            {
                var reason = ex is TaskCanceledException ? "timed out" : ex.Message.TrimEnd('.');
                await Console.Error.WriteLineAsync($"Warning: could not load the key lists from {ListSource()} ({reason}); until they can be reached only the built-in keys are trusted.");
                _published = new PublishedLists(null, ReadVerifiedCache(cacheDir), reason);
            }
            return _published;
        }
        finally
        {
            Gate.Release();
        }
    }

    private static async Task<(string Trusted, string Untrusted, string UntrustedAsc)> FetchAsync()
    {
        var source = ListSource();
        string[] names = [TrustedListName, TrustedListName + ".asc", UntrustedListName, UntrustedListName + ".asc"];
        string[] texts;
        if (source.StartsWith("https://", StringComparison.OrdinalIgnoreCase) || source.StartsWith("http://", StringComparison.OrdinalIgnoreCase))
        {
            var baseUri = new Uri(source.EndsWith('/') ? source : source + "/");
            using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(5) };
            texts = await Task.WhenAll(names.Select(n => http.GetStringAsync(new Uri(baseUri, n))));
        }
        else
        {
            // a local folder, for testing lists before publishing them
            texts = await Task.WhenAll(names.Select(n => File.ReadAllTextAsync(Path.Combine(source, n))));
        }

        RequireListSignature(TrustedListName, texts[0], texts[1]);
        RequireListSignature(UntrustedListName, texts[2], texts[3]);
        return (texts[0], texts[2], texts[3]);
    }

    private static string ListSource() =>
        Environment.GetEnvironmentVariable("TEMPEST_KEYS_URL") is { Length: > 0 } url ? url : DefaultListUrl;

    private static void RequireListSignature(string name, string text, string asc)
    {
        var builtIn = ParseKeyList(ReadBuiltIn(TrustedListName));
        var signers = builtIn.SelectMany(b => b.GetKeyRings()).Select(r => r.GetPublicKey())
            .Where(k => ListSigners.Contains(ModSigning.Fingerprint(k), StringComparer.OrdinalIgnoreCase));
        if (!signers.Any(k => ModSigning.VerifyText(text, asc, k)))
            throw new Exception($"{name} is not signed by a key allowed to sign key lists");
    }

    private static string? ReadVerifiedCache(string cacheDir)
    {
        try
        {
            var text = File.ReadAllText(Path.Combine(cacheDir, UntrustedListName));
            RequireListSignature(UntrustedListName, text, File.ReadAllText(Path.Combine(cacheDir, UntrustedListName + ".asc")));
            return text;
        }
        catch (Exception)
        {
            return null; // never fetched, or tampered with
        }
    }

    private static string ReadBuiltIn(string name)
    {
        using var stream = typeof(TrustStore).Assembly.GetManifestResourceStream(name)
            ?? throw new InvalidOperationException($"built-in {name} is missing from the build");
        using var reader = new StreamReader(stream, Encoding.UTF8);
        return reader.ReadToEnd();
    }

    /// <summary>Every armored public key block in <paramref name="text"/>; unreadable blocks are skipped.</summary>
    private static List<PgpPublicKeyRingBundle> ParseKeyList(string text)
    {
        const string begin = "-----BEGIN PGP PUBLIC KEY BLOCK-----", end = "-----END PGP PUBLIC KEY BLOCK-----";
        var bundles = new List<PgpPublicKeyRingBundle>();
        for (var start = text.IndexOf(begin, StringComparison.Ordinal); start >= 0; start = text.IndexOf(begin, start + begin.Length, StringComparison.Ordinal))
        {
            var stop = text.IndexOf(end, start, StringComparison.Ordinal);
            if (stop < 0) break;
            try
            {
                using var input = PgpUtilities.GetDecoderStream(new MemoryStream(Encoding.UTF8.GetBytes(text[start..(stop + end.Length)])));
                bundles.Add(new PgpPublicKeyRingBundle(input));
            }
            catch (Exception)
            {
                Console.Error.WriteLine("Warning: skipping an unreadable key in a key list.");
            }
        }
        return bundles;
    }

    private static List<string> ParseRevocations(string text)
    {
        var entries = new List<string>();
        foreach (var line in text.Split('\n'))
        {
            var entry = (line.IndexOf('#') is var hash and >= 0 ? line[..hash] : line).Trim().Replace(" ", "");
            if (entry.Length == 0) continue;
            if (entry.Length is 16 or 40 && entry.All(Uri.IsHexDigit))
                entries.Add(entry);
            else
                Console.Error.WriteLine($"Warning: ignoring '{entry}' in the untrusted key list (expected a 40-digit fingerprint or 16-digit key ID).");
        }
        return entries;
    }

    private sealed record PublishedLists(string? Trusted, string? Untrusted, string? OfflineReason);
}
