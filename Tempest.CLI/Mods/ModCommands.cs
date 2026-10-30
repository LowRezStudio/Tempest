using System.Diagnostics;
using System.IO.Compression;
using System.Linq;
using System.Text.Json;
using ConsoleAppFramework;
using Org.BouncyCastle.Bcpg.OpenPgp;
using Tempest.CLI.Extensions;

namespace Tempest.CLI.Mods;

internal class ModCommands
{
    internal static string GetMetadataPath(string gamePath)
    {
        var resolvedGame = GameFolderResolver.Resolve(gamePath);
        var metadataDir = TempestPathUtility.GetLocalModsDirectory(resolvedGame);
        Directory.CreateDirectory(metadataDir);
        return Path.Combine(metadataDir, "mods.json");
    }

    internal static List<ModRecord> LoadMetadata(string gamePath)
    {
        var path = GetMetadataPath(gamePath);
        if (!File.Exists(path)) return [];
        try
        {
            var json = File.ReadAllText(path);
            var mods = JsonSerializer.Deserialize(json, ModSourceGenerationContext.Default.ListModRecord) ?? [];
            // Records from before InstalledAt were appended as they were installed, so their place
            // in the file stands in for the date (and stays before every dated one).
            for (var i = 0; i < mods.Count; i++)
                mods[i].InstalledAt ??= DateTime.UnixEpoch.AddSeconds(i);
            mods = mods.OrderBy(m => m.InstalledAt).ToList();
            var resolvedGame = GameFolderResolver.Resolve(gamePath);
            foreach (var mod in mods)
            {
                for (var i = 0; i < mod.InstalledFiles.Count; i++)
                {
                    var file = mod.InstalledFiles[i];
                    if (!Path.IsPathRooted(file))
                    {
                        mod.InstalledFiles[i] = Path.GetFullPath(Path.Combine(resolvedGame, file));
                    }
                }

                // Migration: initialize OwnedFiles from InstalledFiles for older mods (MetadataVersion < 2)
                // Only for non-INI files (INI files use per-mod ini-backup with merge semantics)
                // Only run once for mods that predate the ownership system (MetadataVersion < 2)
                if (mod.MetadataVersion < 2 && mod.OwnedFiles.Count == 0 && mod.InstalledFiles.Count > 0)
                {
                    mod.OwnedFiles = mod.InstalledFiles
                        .Where(f => Path.GetExtension(f).ToLowerInvariant() != ".ini")
                        .ToList();
                    mod.MetadataVersion = 2;
                }

                if (!string.Equals(mod.Kind, "V2", StringComparison.OrdinalIgnoreCase)) continue;
                
                var modDir = TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, mod.Id);
                if (!Directory.Exists(modDir)) continue;
                    
                string? foundReadmeFile = null;
                if (!string.IsNullOrEmpty(mod.Readme) && File.Exists(Path.Combine(modDir, mod.Readme)))
                {
                    foundReadmeFile = mod.Readme;
                }
                else
                {
                    var fallbacks = new[] { "README.md", "readme.md", "README.txt", "readme.txt" };
                    foreach (var fallback in fallbacks)
                    {
                        if (!File.Exists(Path.Combine(modDir, fallback))) continue;
                                
                        foundReadmeFile = fallback;
                        mod.Readme = fallback;
                        break;
                    }
                }

                if (foundReadmeFile == null) continue;
                    
                try
                {
                    mod.ReadmeContent = File.ReadAllText(Path.Combine(modDir, foundReadmeFile));
                }
                catch
                {
                    mod.ReadmeContent = "Error reading readme file.";
                }
            }
            return mods;
        }
        catch
        {
            return [];
        }
    }

    internal static void SaveMetadata(string gamePath, List<ModRecord> mods)
    {
        var path = GetMetadataPath(gamePath);
        try
        {
            var resolvedGame = GameFolderResolver.Resolve(gamePath);
            foreach (var mod in mods)
            {
                for (var i = 0; i < mod.InstalledFiles.Count; i++)
                {
                    var file = mod.InstalledFiles[i];
                    if (Path.IsPathRooted(file))
                    {
                        mod.InstalledFiles[i] = Path.GetRelativePath(resolvedGame, file);
                    }
                }
            }
            var json = JsonSerializer.Serialize(mods, ModSourceGenerationContext.Default.ListModRecord);
            File.WriteAllText(path, json);

            // ponytail: restore absolute paths in memory to avoid breaking subsequent operations/printing
            foreach (var mod in mods)
            {
                for (var i = 0; i < mod.InstalledFiles.Count; i++)
                {
                    var file = mod.InstalledFiles[i];
                    if (!Path.IsPathRooted(file))
                    {
                        mod.InstalledFiles[i] = Path.GetFullPath(Path.Combine(resolvedGame, file));
                    }
                }
            }
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Failed to save mod metadata: {ex.Message}");
        }
    }

    /// <summary>Installs a mod into the game instance</summary>
    /// <param name="path">Path to the game folder or executable</param>
    /// <param name="modFile">Path to the mod file (.upk, .pck)</param>
    /// <param name="replace">Overwrite the mod if it already exists</param>
    /// <param name="stack">Add mod alongside conflicting mods, transferring ownership of conflicting files</param>
    /// <param name="allowUntrusted">Install code (DLLs, Lua) signed by a key that is not trusted</param>
    /// <param name="json">Output as JSON</param>
    /// <param name="patch">When modFile is a repository id, pick the newest version compatible with this Paladins patch (e.g. 0.57)</param>
    /// <param name="noDeps">When modFile is a repository id, do not install the dependencies it declares</param>
    /// <param name="ignoreTempestVersion">Install even when the package needs a newer Tempest than this one</param>
    /// <param name="tempestVersion">The Tempest version to check "tempest" requirements against (the launcher passes its own; default: the version this CLI was built with)</param>
    /// <param name="defer">Run every check and stop before writing anything, reporting Deferred (the launcher queues installs this way while the game runs)</param>
    public async Task Install([Argument] string path, [Argument] string modFile, bool replace = false, bool stack = false, bool allowUntrusted = false, bool json = false, string? patch = null, bool noDeps = false, bool ignoreTempestVersion = false, string? tempestVersion = null, bool defer = false)
    {
        if (!string.IsNullOrWhiteSpace(tempestVersion)) Environment.SetEnvironmentVariable("TEMPEST_VERSION", tempestVersion);
        try
        {
            string? downloaded = null;
            var installedDeps = new List<string>();
            // Entered by the installer once the package is verified; held through the
            // dependencies and the mod itself, so another launcher install cannot interleave.
            using var modLock = new ModLock(path);
            if (!File.Exists(modFile))
            {
                // Not a local file: an http(s) URL, or a repository id ("vexa-crosshair" or "vexa-crosshair@1.3.0").
                RepoPick? pick;
                try
                {
                    (downloaded, pick) = await FetchFromRepoAsync(modFile, patch);
                }
                catch (Exception ex)
                {
                    PrintResult(new ModInstallResult { Success = false, Message = $"Mod file not found: {modFile} ({ex.Message})" }, json);
                    return;
                }
                if (downloaded == null)
                {
                    PrintResult(new ModInstallResult { Success = false, Message = $"Mod file not found: {modFile}" }, json);
                    return;
                }
                modFile = downloaded;
                // What the version declares it needs goes in first, leaves of the tree before
                // the mods that need them, so the mod itself never reports a missing dependency.
                if (pick != null && !noDeps && !defer)
                {
                    var failure = await InstallDependenciesAsync(path, pick, patch, allowUntrusted, ignoreTempestVersion, installedDeps, modLock);
                    if (failure != null)
                    {
                        PrintResult(failure, json);
                        File.Delete(downloaded);
                        return;
                    }
                }
            }

            var result = await InstallOneAsync(path, modFile, replace, stack, allowUntrusted, ignoreTempestVersion, modLock, defer);
            if (installedDeps.Count > 0) result.InstalledDependencies = installedDeps;

            PrintResult(result, json);
            if (downloaded != null) File.Delete(downloaded);
        }
        catch (Exception ex)
        {
            var fail = new ModInstallResult { Success = false, Message = ex.Message };
            PrintResult(fail, json);
        }
    }

    /// <summary>Installs one package under <paramref name="modLock"/> and records it; the install result is returned as is.</summary>
    private static async Task<ModInstallResult> InstallOneAsync(string path, string modFile, bool replace, bool stack, bool allowUntrusted, bool ignoreTempestVersion, ModLock modLock, bool defer = false)
    {
        var installer = ModInstallerFactory.CreateForFile(modFile);
        // Legacy packages have no checks to run ahead; deferring them only reports so.
        if (defer && installer is not ModV2Installer)
            return new ModInstallResult { Success = false, Deferred = true, Message = $"'{Path.GetFileName(modFile)}' was not installed (--defer)." };
        // Mods an install replaces give their files back, which the running game must accept again too.
        var filesBefore = ModGameFiles(LoadMetadata(path)).ToList();
        var result = installer is ModV2Installer v2
            ? await v2.InstallAsync(path, modFile, replace, stack, allowUntrusted, modLock, ignoreTempestVersion, defer)
            : await installer.InstallAsync(path, modFile, replace, stack, allowUntrusted, modLock);

        if (result.Success && result.Mod != null)
        {
            RecordInstall(path, result.Mod);
            // Delta-managed game files are rebuilt from the enabled mods, including this one.
            await DeltaResolver.ResolveAndReportAsync(path);
            Launcher.PackageHashes.ApplyToRunning(path, filesBefore.Concat(ModGameFiles(LoadMetadata(path))));
        }
        return result;
    }

    /// <summary>The game files the mods replace or rebuild from deltas, enabled or not.</summary>
    private static IEnumerable<string> ModGameFiles(IEnumerable<ModRecord> mods) =>
        mods.SelectMany(m => m.OwnedFiles.Concat(m.DeltaFiles));

    /// <summary>Records an installed mod in place of one with its name, keeping that one's install date and place in the list.</summary>
    private static void RecordInstall(string path, ModRecord mod)
    {
        var mods = LoadMetadata(path);
        var replaced = mods.FirstOrDefault(m => string.Equals(m.Name, mod.Name, StringComparison.OrdinalIgnoreCase));
        if (replaced != null) mod.InstalledAt = replaced.InstalledAt;
        mods.RemoveAll(m => string.Equals(m.Name, mod.Name, StringComparison.OrdinalIgnoreCase));
        var at = mods.FindIndex(m => m.InstalledAt > mod.InstalledAt);
        mods.Insert(at < 0 ? mods.Count : at, mod);
        SaveMetadata(path, mods);
    }

    /// <summary>A version picked on the repository, so its dependencies can be looked up.</summary>
    private sealed record RepoPick(string Id, string Version);

    /// <summary>
    /// Installs what <paramref name="pick"/> declares it needs and does not have yet, as the
    /// repository resolves it. A dependency the repository cannot resolve is only reported on
    /// stderr (the mod's own install result lists it as missing). Returns the failing install
    /// result when a dependency refuses to install, else null.
    /// </summary>
    private static async Task<ModInstallResult?> InstallDependenciesAsync(string path, RepoPick pick, string? patch, bool allowUntrusted, bool ignoreTempestVersion, List<string> installed, ModLock modLock)
    {
        RepoResolution resolution;
        using var repo = new ModRepoClient();
        try
        {
            resolution = await repo.GetDependenciesAsync(pick.Id, pick.Version, patch, ModDependencies.TempestVersion);
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Warning: could not resolve the dependencies of {pick.Id}: {ex.Message}");
            return null;
        }
        if (resolution.Items.Count == 0) return null;
        await modLock.EnterAsync();
        var mods = LoadMetadata(path);
        // Breadth first from the mod: reversed, a dependency's own dependencies land first.
        foreach (var item in Enumerable.Reverse(resolution.Items))
        {
            var current = mods.FirstOrDefault(m => string.Equals(m.Id, item.Id, StringComparison.OrdinalIgnoreCase));
            if (current is { Enabled: true } && item.Requirements.All(r => SemverRange.Satisfies(current.Version, r))) continue;
            if (item.Resolved == null)
            {
                var why = item.Problem == "not_found" ? "is not on the repository" : "has no published version that fits";
                await Console.Error.WriteLineAsync($"Warning: {pick.Id} needs {item.Id} ({string.Join(", ", item.Requirements.Select(r => r.Length == 0 ? "any version" : r))}), which {why}.");
                continue;
            }
            if (current is { Enabled: false } && SemverRange.Satisfies(current.Version, string.Join(" ", item.Requirements)))
            {
                // Installed at a fitting version, only switched off.
                await CreateInstaller(current).EnableAsync(path, current);
                current.Enabled = true;
                current.Verified = false;
                SaveMetadata(path, mods);
                installed.Add($"{current.Id}@{current.Version}");
                continue;
            }
            var dest = Path.Combine(Path.GetTempPath(), $"tempest-{Guid.NewGuid():N}.tempest");
            try
            {
                await Console.Error.WriteLineAsync($"Installing dependency {item.Id} {item.Resolved.Version} (needed by {string.Join(", ", item.RequiredBy)})");
                await repo.DownloadAsync(item.Id, item.Resolved.Version, dest);
                var result = await InstallOneAsync(path, dest, replace: true, stack: false, allowUntrusted, ignoreTempestVersion, modLock);
                if (!result.Success)
                {
                    result.Message = $"dependency {item.Id} {item.Resolved.Version}: {result.Message}";
                    return result;
                }
                installed.Add($"{item.Id}@{item.Resolved.Version}");
                mods = LoadMetadata(path);
            }
            finally
            {
                if (File.Exists(dest)) File.Delete(dest);
            }
        }
        return null;
    }

    /// <summary>
    /// Resolves a URL or a repository id (optionally "id@version") to a downloaded package in the
    /// temp folder (with the id and version when it came from the repository), or null when the
    /// spec is neither.
    /// </summary>
    private static async Task<(string? File, RepoPick? Pick)> FetchFromRepoAsync(string spec, string? patch)
    {
        var dest = Path.Combine(Path.GetTempPath(), $"tempest-{Guid.NewGuid():N}.tempest");
        if (spec.StartsWith("http://", StringComparison.OrdinalIgnoreCase) || spec.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
        {
            using var http = new HttpClient { Timeout = TimeSpan.FromMinutes(30) };
            using var res = await http.GetAsync(spec, HttpCompletionOption.ResponseHeadersRead);
            res.EnsureSuccessStatusCode();
            await using var file = File.Create(dest);
            await res.Content.CopyToAsync(file);
            return (dest, null);
        }
        var at = spec.IndexOf('@');
        var id = at < 0 ? spec : spec[..at];
        var wanted = at < 0 ? null : spec[(at + 1)..];
        if (!System.Text.RegularExpressions.Regex.IsMatch(id, "^[a-z0-9][a-z0-9._-]{1,63}$")) return (null, null);

        using var repo = new ModRepoClient();
        var versions = (await repo.GetVersionsAsync(id)).Where(v => v.Status == "approved").ToList();
        if (versions.Count == 0) throw new Exception($"no published version of {id} on {repo.BaseUri.Host}");
        RepoVersion? pick;
        if (wanted != null)
            pick = versions.FirstOrDefault(v => v.Version == wanted) ?? throw new Exception($"{id} has no version {wanted}");
        else
        {
            var tempest = ModDependencies.TempestVersion;
            var candidates = versions
                .Where(v => patch == null || Compatible(v.Compat, patch))
                .Where(v => ModDependencies.UnmetTempestRequirement(v.Dependencies.Select(d => new ModDependency { Id = d.Id, Requirement = d.Requirement }), tempest) == null)
                .ToList();
            if (candidates.Count == 0) throw new Exception(patch == null ? $"no version of {id} runs on Tempest {tempest}" : $"no version of {id} is compatible with patch {patch}" + (tempest == null ? "" : $" and Tempest {tempest}"));
            pick = candidates.OrderByDescending(v => v.Version, SemverComparer.Instance).First();
        }
        await repo.DownloadAsync(id, pick.Version, dest);
        return (dest, new RepoPick(id, pick.Version));
    }

    /// <summary>Mirrors src/lib/compat.ts on the repository: explicit patches or inclusive ranges, numeric by component.</summary>
    internal static bool Compatible(RepoCompat compat, string patch)
    {
        if (compat.Patches.Contains(patch)) return true;
        var n = PatchOrder(patch);
        return compat.Ranges.Any(r => (r.From == null || PatchOrder(r.From) <= n) && (r.To == null || n <= PatchOrder(r.To)));
    }

    private static long PatchOrder(string version)
    {
        var parts = version.Split('.');
        long Part(int i) => parts.Length > i && long.TryParse(parts[i], out var v) ? v : 0;
        return Part(0) * 1_000_000 + Part(1) * 10_000 + Part(2);
    }

    /// <summary>Searches the mod repository</summary>
    /// <param name="query">Words to match in name, summary, id, tags or author</param>
    /// <param name="patch">Only mods compatible with this Paladins patch (e.g. 0.57)</param>
    /// <param name="kind">assets, scripts or native</param>
    /// <param name="json">Output as JSON</param>
    public async Task<int> Search([Argument] string? query = null, string? patch = null, string? kind = null, bool json = false)
    {
        try
        {
            using var repo = new ModRepoClient();
            var page = await repo.SearchAsync(query, patch, kind);
            if (json)
            {
                Console.WriteLine(JsonSerializer.Serialize(page, RepoJsonContext.Default.RepoModPage));
                return 0;
            }
            if (page.Items.Count == 0)
            {
                Console.WriteLine("No mods found.");
                return 0;
            }
            foreach (var mod in page.Items)
            {
                var latest = mod.Latest == null ? "" : $" v{mod.Latest.Version}";
                var trust = mod.Latest?.Signer?.Trust switch { "first-party" => " [Tempest team]", "trusted" => " [trusted]", "unknown" => " [unknown signer]", _ => "" };
                Console.WriteLine($"{mod.Id}{latest}  {mod.Name} by {mod.Owner.DisplayName}  ({mod.Kind}{trust}, {mod.Downloads} installs)");
                Console.WriteLine($"    {mod.Summary}");
            }
            if (page.Total > page.Items.Count) Console.WriteLine($"... {page.Total} results, showing {page.Items.Count}. See {repo.BaseUri}mods");
            return 0;
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Search failed: {ex.Message}");
            return 1;
        }
    }

    /// <summary>Stores an API key for publishing (create one on the website under Settings)</summary>
    /// <param name="apiKey">The tm_live_... key; omit to forget the stored one</param>
    /// <param name="url">Repository URL, for a self-hosted instance</param>
    public async Task<int> Login([Argument] string? apiKey = null, string? url = null)
    {
        var config = RepoConfig.Load();
        if (apiKey == null && url == null)
        {
            config.ApiKey = null;
            config.Save();
            Console.WriteLine("Forgot the stored API key.");
            return 0;
        }
        if (apiKey != null) config.ApiKey = apiKey;
        if (url != null) config.Url = url.TrimEnd('/');
        config.Save();
        try
        {
            using var repo = new ModRepoClient();
            var me = await repo.MeAsync();
            Console.WriteLine($"Signed in to {repo.BaseUri.Host} as {me.DisplayName} (@{me.Handle}, {me.Role}{(me.Team ? ", team" : "")}). Key stored in {RepoConfig.Path}");
            return 0;
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"The key was stored but does not work: {ex.Message}");
            return 1;
        }
    }

    /// <summary>Uploads a signed .tempest package to the mod repository</summary>
    /// <param name="package">The package (from `mod pack` + `mod sign`)</param>
    /// <param name="patch">Compatible patch, repeatable (e.g. --patch 0.56 --patch 0.57)</param>
    /// <param name="from">Start of a compatible patch range (inclusive)</param>
    /// <param name="to">End of a compatible patch range (inclusive); omit for "and later"</param>
    /// <param name="summary">One-line summary; required for a new mod</param>
    /// <param name="tags">Comma-separated tags</param>
    /// <param name="changelog">What changed in this version</param>
    /// <param name="repository">Source repository URL</param>
    /// <param name="json">Output as JSON</param>
    public async Task<int> Publish([Argument] string package, string[]? patch = null, string? from = null, string? to = null, string? summary = null, string? tags = null, string? changelog = null, string? repository = null, bool json = false)
    {
        try
        {
            if (!File.Exists(package)) throw new FileNotFoundException("package not found", package);
            var compat = new RepoCompat { Patches = [.. patch ?? []] };
            if (from != null || to != null) compat.Ranges.Add(new RepoCompatRange { From = from, To = to });
            if (compat.Patches.Count == 0 && compat.Ranges.Count == 0) throw new Exception("declare compatibility: --patch <version> (repeatable), or --from <version> [--to <version>]");
            var listing = new RepoListing
            {
                Compat = compat,
                Summary = summary,
                Changelog = changelog,
                Repository = repository,
                Tags = tags?.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).Select(t => t.ToLowerInvariant()).ToList(),
            };
            using var repo = new ModRepoClient();
            var result = await repo.PublishAsync(package, listing, json ? null : s => Console.Error.WriteLine($"  {s}..."));
            if (json)
            {
                Console.WriteLine(JsonSerializer.Serialize(result, RepoJsonContext.Default.RepoIntent));
                return result.State == "done" ? 0 : 1;
            }
            if (result.State == "failed")
            {
                Console.Error.WriteLine("Not accepted:");
                foreach (var e in result.Result?.Errors ?? []) Console.Error.WriteLine($"  - {e}");
                return 1;
            }
            Console.WriteLine($"Uploaded {result.Result?.ModId} v{result.Result?.Version}.");
            foreach (var w in result.Result?.Warnings ?? []) Console.WriteLine($"  warning: {w}");
            var versions = await repo.GetVersionsAsync(result.Result?.ModId ?? "");
            var status = versions.FirstOrDefault(v => v.Id == result.VersionId)?.Status;
            Console.WriteLine(status switch
            {
                "approved" => $"Published: {repo.BaseUri}mods/{result.Result?.ModId}",
                "pending_review" => "Queued for moderation; the dashboard shows the outcome.",
                "rejected" => "Rejected by inspection; see the dashboard for the reasons.",
                _ => $"Status: {status ?? "unknown"}",
            });
            return status == "rejected" ? 1 : 0;
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Publish failed: {ex.Message}");
            return 1;
        }
    }

    /// <summary>Lists installed mods for the game instance</summary>
    /// <param name="path">Path to the game folder or executable</param>
    /// <param name="json">Output as JSON</param>
    public async Task List([Argument] string path, bool json = false)
    {
        try
        {
            // Installs running alongside rewrite mods.json; wait rather than read it half-written.
            List<ModRecord> mods;
            using (await ModLock.AcquireAsync(path)) mods = LoadMetadata(path);
            if (json)
            {
                var result = new ModListResult { Mods = mods };
                Console.WriteLine(JsonSerializer.Serialize(result, ModSourceGenerationContext.Default.ModListResult));
            }
            else
            {
                Console.WriteLine("Installed Mods:");
                if (mods.Count == 0)
                {
                    Console.WriteLine("  No mods installed.");
                }
                foreach (var mod in mods)
                {
                    Console.WriteLine($"  - {mod.Name} (Kind: {mod.Kind}, Enabled: {mod.Enabled})");
                }
            }
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Error: {ex.Message}");
        }
    }

    /// <summary>Reloads DLLs for a V2 mod into a running game process</summary>
    /// <param name="path">Path to the game folder or executable</param>
    /// <param name="modId">ID of the mod whose DLLs to reload</param>
    /// <param name="json">Output as JSON</param>
    public async Task ReloadDll([Argument] string path, [Argument] string modId, bool json = false)
    {
        try
        {
            var resolvedGame = GameFolderResolver.Resolve(path);
            var binaries64 = Path.Combine(resolvedGame, "Binaries", "Win64");
            var exeFiles = Directory.Exists(binaries64)
                ? Directory.GetFiles(binaries64, "*.exe", SearchOption.TopDirectoryOnly)
                : Directory.GetFiles(Path.Combine(resolvedGame, "Binaries"), "*.exe", SearchOption.AllDirectories);
            var gameExe = exeFiles.FirstOrDefault(f =>
            {
                var name = Path.GetFileNameWithoutExtension(f);
                return !name.Contains("AutoReporter", StringComparison.OrdinalIgnoreCase)
                    && !name.Contains("CrashReport", StringComparison.OrdinalIgnoreCase)
                    && !name.Contains("Launcher", StringComparison.OrdinalIgnoreCase);
            }) ?? exeFiles.FirstOrDefault();
            if (gameExe == null)
            {
                var fail = new ModInstallResult { Success = false, Message = "No game executable found in Binaries directory." };
                PrintResult(fail, json);
                return;
            }

            var exeName = Path.GetFileNameWithoutExtension(gameExe);
            var is64Bit = gameExe.Contains("Win64", StringComparison.OrdinalIgnoreCase);

            var processes = Process.GetProcessesByName(exeName)
                .Where(p => !p.HasExited)
                .ToArray();
            if (processes.Length == 0)
            {
                var fail = new ModInstallResult { Success = false, Message = $"No running process found for '{exeName}'." };
                PrintResult(fail, json);
                return;
            }

            var process = processes[0];
            Console.Error.WriteLine($"[reload] Targeting PID {process.Id}");

            var dllsDir = Path.Combine(TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, modId), "dlls");
            if (!Directory.Exists(dllsDir))
            {
                var fail = new ModInstallResult { Success = false, Message = $"No DLL directory found for mod '{modId}'." };
                PrintResult(fail, json);
                return;
            }

            var dlls = Launcher.ModDlls.ForProcess(dllsDir, is64Bit);
            if (dlls.Count == 0)
            {
                var fail = new ModInstallResult { Success = false, Message = $"No DLL files for a {(is64Bit ? "Win64" : "Win32")} game found for mod '{modId}'." };
                PrintResult(fail, json);
                return;
            }

            var results = await Task.WhenAll(dlls.Select(d => process.InjectLibraryAsync(d, is64Bit)));
            var allSucceeded = results.All(r => r);
            var ok = new ModInstallResult
            {
                Success = allSucceeded,
                Message = allSucceeded
                    ? $"Successfully reloaded {dlls.Count} DLL(s) for mod '{modId}'."
                    : $"Failed to reload some DLLs for mod '{modId}'."
            };
            PrintResult(ok, json);
        }
        catch (Exception ex)
        {
            var fail = new ModInstallResult { Success = false, Message = ex.Message };
            PrintResult(fail, json);
        }
    }

    /// <summary>Removes a mod from the game instance</summary>
    /// <param name="path">Path to the game folder or executable</param>
    /// <param name="modName">Name of the mod to remove (e.g. Tempest Mod.tempest)</param>
    /// <param name="json">Output as JSON</param>
    public async Task Remove([Argument] string path, [Argument] string modName, bool json = false)
    {
        try
        {
            using var modLock = await ModLock.AcquireAsync(path);
            var mods = LoadMetadata(path);
            var mod = mods.FirstOrDefault(m => string.Equals(m.Name, modName, StringComparison.OrdinalIgnoreCase));

            if (mod == null)
            {
                var fail = new ModInstallResult { Success = false, Message = $"Mod not found: {modName}" };
                PrintResult(fail, json);
                return;
            }

            var dependants = ModDependencies.Dependants(mod.Id, mods);
            var installer = CreateInstaller(mod);
            await installer.RemoveAsync(path, mod);
            mods.Remove(mod);
            SaveMetadata(path, mods);
            await DeltaResolver.ResolveAndReportAsync(path);
            Launcher.PackageHashes.ApplyToRunning(path, ModGameFiles([.. mods, mod]));

            var note = dependants.Count == 0 ? "" : $" {string.Join(", ", dependants.Select(d => $"'{d.Name}'"))} depend{(dependants.Count == 1 ? "s" : "")} on it.";
            var ok = new ModInstallResult { Success = true, Message = $"Mod '{modName}' removed successfully.{note}" };
            PrintResult(ok, json);
        }
        catch (Exception ex)
        {
            var fail = new ModInstallResult { Success = false, Message = ex.Message };
            PrintResult(fail, json);
        }
    }

    /// <summary>Enables a mod in the game instance, applying its files back to the game directory</summary>
    /// <param name="path">Path to the game folder or executable</param>
    /// <param name="modName">Name of the mod to enable</param>
    /// <param name="json">Output as JSON</param>
    public async Task Enable([Argument] string path, [Argument] string modName, bool json = false)
    {
        try
        {
            using var modLock = await ModLock.AcquireAsync(path);
            var mods = LoadMetadata(path);
            var mod = mods.FirstOrDefault(m => string.Equals(m.Name, modName, StringComparison.OrdinalIgnoreCase));

            if (mod == null)
            {
                var fail = new ModInstallResult { Success = false, Message = $"Mod not found: {modName}" };
                PrintResult(fail, json);
                return;
            }

            if (mod.Enabled)
            {
                var ok = new ModInstallResult { Success = true, Message = $"Mod '{modName}' is already enabled." };
                PrintResult(ok, json);
                return;
            }

            var installer = CreateInstaller(mod);
            await installer.EnableAsync(path, mod);
            mod.Enabled = true;
            // Files were just re-applied from an unverified snapshot; the next launch
            // re-checks them against the mod's signed CHECKSUMS first.
            mod.Verified = false;
            SaveMetadata(path, mods);
            await DeltaResolver.ResolveAndReportAsync(path);
            Launcher.PackageHashes.ApplyToRunning(path, ModGameFiles(mods));

            var result = new ModInstallResult { Success = true, Message = $"Mod '{modName}' enabled successfully." };
            PrintResult(result, json);
        }
        catch (Exception ex)
        {
            var fail = new ModInstallResult { Success = false, Message = ex.Message };
            PrintResult(fail, json);
        }
    }

    /// <summary>Disables a mod in the game instance, reverting its file changes while keeping the mod data</summary>
    /// <param name="path">Path to the game folder or executable</param>
    /// <param name="modName">Name of the mod to disable</param>
    /// <param name="json">Output as JSON</param>
    public async Task Disable([Argument] string path, [Argument] string modName, bool json = false)
    {
        try
        {
            using var modLock = await ModLock.AcquireAsync(path);
            var mods = LoadMetadata(path);
            var mod = mods.FirstOrDefault(m => string.Equals(m.Name, modName, StringComparison.OrdinalIgnoreCase));

            if (mod == null)
            {
                var fail = new ModInstallResult { Success = false, Message = $"Mod not found: {modName}" };
                PrintResult(fail, json);
                return;
            }

            if (!mod.Enabled)
            {
                var ok = new ModInstallResult { Success = true, Message = $"Mod '{modName}' is already disabled." };
                PrintResult(ok, json);
                return;
            }

            var dependants = ModDependencies.Dependants(mod.Id, mods);
            var installer = CreateInstaller(mod);
            await installer.DisableAsync(path, mod);
            mod.Enabled = false;
            SaveMetadata(path, mods);
            await DeltaResolver.ResolveAndReportAsync(path);
            Launcher.PackageHashes.ApplyToRunning(path, ModGameFiles(mods));

            var note = dependants.Count == 0 ? "" : $" {string.Join(", ", dependants.Select(d => $"'{d.Name}'"))} depend{(dependants.Count == 1 ? "s" : "")} on it.";
            var result = new ModInstallResult { Success = true, Message = $"Mod '{modName}' disabled successfully.{note}" };
            PrintResult(result, json);
        }
        catch (Exception ex)
        {
            var fail = new ModInstallResult { Success = false, Message = ex.Message };
            PrintResult(fail, json);
        }
    }

    /// <summary>Disables every enabled mod whose signing key is on the untrusted list (DLLs and Lua alike).</summary>
    internal static async Task DisableRevokedModsAsync(string path)
    {
        try
        {
            if (!File.Exists(GetMetadataPath(path))) return;
            using var modLock = await ModLock.AcquireAsync(path);
            var mods = LoadMetadata(path);
            var revoked = new List<ModRecord>();
            TrustStore? trust = null;
            foreach (var mod in mods.Where(m => m.Enabled && m.Signer.Length > 0))
            {
                trust ??= await TrustStore.LoadAsync(GameFolderResolver.Resolve(path));
                if (trust.IsRevoked(mod.Signer)) revoked.Add(mod);
            }
            if (revoked.Count == 0) return;

            foreach (var mod in revoked)
            {
                await CreateInstaller(mod).DisableAsync(path, mod);
                mod.Enabled = false;
                await Console.Error.WriteLineAsync($"Warning: disabled '{mod.Name}': the key it was signed with ({mod.Signer}) has been revoked.");
            }
            SaveMetadata(path, mods);
            await DeltaResolver.ResolveAndReportAsync(path);
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Warning: could not check installed mods for revoked keys: {ex.Message}");
        }
    }

    internal static IModInstaller CreateInstaller(ModRecord mod)
    {
        if (string.Equals(mod.Kind, "V2", StringComparison.OrdinalIgnoreCase))
            return new ModV2Installer();

        try
        {
            return ModInstallerFactory.CreateForFile(mod.Name);
        }
        catch
        {
            return new ModV1Installer();
        }
    }

    /// <summary>Renames a mod in the game instance</summary>
    /// <param name="path">Path to the game folder or executable</param>
    /// <param name="oldName">Old name of the mod</param>
    /// <param name="newName">New name of the mod</param>
    /// <param name="json">Output as JSON</param>
    public async Task Rename([Argument] string path, [Argument] string oldName, [Argument] string newName, bool json = false)
    {
        try
        {
            using var modLock = await ModLock.AcquireAsync(path);
            var mods = LoadMetadata(path);
            var mod = mods.FirstOrDefault(m => string.Equals(m.Name, oldName, StringComparison.OrdinalIgnoreCase));

            if (mod == null)
            {
                var fail = new ModInstallResult { Success = false, Message = $"Mod not found: {oldName}" };
                PrintResult(fail, json);
                return;
            }

            mod.Name = newName;
            SaveMetadata(path, mods);

            var ok = new ModInstallResult { Success = true, Message = $"Mod renamed to '{newName}' successfully.", Mod = mod };
            PrintResult(ok, json);
        }
        catch (Exception ex)
        {
            var fail = new ModInstallResult { Success = false, Message = ex.Message };
            PrintResult(fail, json);
        }
    }

    /// <summary>Installs multiple mods into the game instance</summary>
    /// <param name="path">Path to the game folder or executable</param>
    /// <param name="modFiles">List of mod files to install</param>
    /// <param name="replace">Overwrite existing mods</param>
    /// <param name="stack">Add mods alongside conflicting mods, transferring ownership of conflicting files</param>
    /// <param name="allowUntrusted">Install code (DLLs, Lua) signed by a key that is not trusted</param>
    /// <param name="json">Output as JSON</param>
    public async Task InstallBulk([Argument] string path, string[] modFiles, bool replace = false, bool stack = false, bool allowUntrusted = false, bool json = false)
    {
        var results = new List<ModInstallResult>();
        using var modLock = new ModLock(path);
        foreach (var file in modFiles)
        {
            try
            {
                if (!File.Exists(file))
                {
                    results.Add(new ModInstallResult { Success = false, Message = $"Mod file not found: {file}" });
                    continue;
                }

                var installer = ModInstallerFactory.CreateForFile(file);
                var result = await installer.InstallAsync(path, file, replace, stack, allowUntrusted, modLock);

                if (result.Success && result.Mod != null) RecordInstall(path, result.Mod);
                results.Add(result);
            }
            catch (Exception ex)
            {
                results.Add(new ModInstallResult { Success = false, Message = $"{Path.GetFileName(file)}: {ex.Message}" });
            }
        }

        if (json)
        {
            var bulkResult = new ModBulkResult { Results = results };
            Console.WriteLine(JsonSerializer.Serialize(bulkResult, ModSourceGenerationContext.Default.ModBulkResult));
        }
        else
        {
            foreach (var res in results)
            {
                Console.WriteLine($"{(res.Success ? "Success" : "Failed")}: {res.Message}");
            }
        }
    }

    private static void PrintResult(ModInstallResult result, bool json)
    {
        if (json)
        {
            Console.WriteLine(JsonSerializer.Serialize(result, ModSourceGenerationContext.Default.ModInstallResult));
        }
        else
        {
            if (result.Success)
            {
                Console.WriteLine($"Success: {result.Message}");
            }
            else if (result.Unverified)
            {
                Console.WriteLine($"Unverified: {result.Message}");
            }
            else if (result.Conflict)
            {
                Console.WriteLine($"Conflict: {result.Message}");
            }
            else
            {
                Console.WriteLine($"Error: {result.Message}");
            }
        }
    }

    /// <summary>Packages a mod directory into a .tempest mod package, optionally signing it</summary>
    /// <param name="sourceDir">Path to the mod folder containing manifest.toml, files/, dlls/ etc</param>
    /// <param name="outputZip">Path to the output .tempest or .zip file</param>
    /// <param name="key">Private key file to sign the mod with (from `mod gen-key`)</param>
    public async Task<int> Pack([Argument] string sourceDir, [Argument] string outputZip, string? key = null)
    {
        try
        {
            var fullSourceDir = Path.GetFullPath(sourceDir);
            if (!File.Exists(Path.Combine(fullSourceDir, "manifest.toml")))
            {
                await Console.Error.WriteLineAsync($"Error: manifest.toml not found in source directory: {sourceDir}");
                return 1;
            }

            var secretKey = key == null ? null : ModSigning.ReadSecretKey(await File.ReadAllTextAsync(key));

            if (File.Exists(outputZip)) File.Delete(outputZip);
            using (var archive = ZipFile.Open(outputZip, ZipArchiveMode.Create))
            {
                foreach (var file in Directory.GetFiles(fullSourceDir, "*", SearchOption.AllDirectories))
                {
                    var relativePath = Path.GetRelativePath(fullSourceDir, file).Replace('\\', '/');
                    if (!ModSigning.IsSealEntry(relativePath))
                        archive.CreateEntryFromFile(file, relativePath);
                }
            }
            // Sealing reads the entries back, which a Create-mode archive cannot do.
            using (var archive = ZipFile.Open(outputZip, ZipArchiveMode.Update))
            {
                ModSigning.Seal(archive, secretKey);
            }

            Console.WriteLine(secretKey == null
                ? $"Packed '{outputZip}' (unsigned)"
                : $"Packed '{outputZip}', signed by {ModSigning.Describe(secretKey.PublicKey)}");
            return 0;
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Failed to pack mod: {ex.Message}");
            return 1;
        }
    }

    /// <summary>Signs an existing .tempest package in place, rewriting its CHECKSUMS and CHECKSUMS.asc</summary>
    /// <param name="package">Path to the .tempest package</param>
    /// <param name="key">Private key file to sign with; defaults to $TEMPEST_SIGNING_KEY, then the one `mod gen-key` writes by default</param>
    public async Task<int> Sign([Argument] string package, string? key = null)
    {
        try
        {
            var secretKey = ModSigning.ReadSecretKey(await File.ReadAllTextAsync(key ?? TempestPathUtility.GetDefaultSigningKeyPath()));
            using (var archive = ZipFile.Open(package, ZipArchiveMode.Update))
            {
                if (archive.GetEntry("manifest.toml") == null)
                    throw new Exception("not a .tempest package (no manifest.toml)");
                ModSigning.Seal(archive, secretKey);
            }
            Console.WriteLine($"Signed '{package}' with {ModSigning.Describe(secretKey.PublicKey)}");
            return 0;
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Failed to sign mod: {ex.Message}");
            return 1;
        }
    }

    /// <summary>Checks a package's signature against the trusted and revoked keys; exits 0 only when trusted</summary>
    /// <param name="package">Path to the .tempest package</param>
    /// <param name="gamePath">Game folder whose .tempest/keys should be trusted as well</param>
    public async Task<int> Verify([Argument] string package, string? gamePath = null)
    {
        try
        {
            var trust = await TrustStore.LoadAsync(gamePath == null ? null : GameFolderResolver.Resolve(gamePath));
            using var archive = ZipFile.OpenRead(package);
            var checksums = archive.GetEntry(ModSigning.ChecksumsName);
            if (checksums == null)
            {
                Console.WriteLine("unsigned (no CHECKSUMS)");
                return 1;
            }

            var result = ModSigning.Verify(
                await ReadEntryAsync(checksums),
                archive.GetEntry(ModSigning.SignatureName) is { } asc ? await ReadEntryAsync(asc) : null,
                archive.GetEntry(ModSigning.SignerKeyName) is { } key ? await ReadEntryAsync(key) : null,
                trust);
            Console.WriteLine(result.Trust switch
            {
                ModSigning.Trust.Unsigned => "unsigned (no CHECKSUMS.asc)",
                ModSigning.Trust.Invalid => $"invalid: {result.Problem}",
                ModSigning.Trust.Untrusted => $"untrusted: signed by {result.Signer}",
                ModSigning.Trust.Revoked => $"revoked: signed by {result.Signer}",
                _ => $"trusted: signed by {result.Signer}",
            });
            return result.Trust == ModSigning.Trust.Trusted ? 0 : 1;
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Failed to verify mod: {ex.Message}");
            return 1;
        }
    }

    /// <summary>Shows where the key lists come from, whether they could be loaded, and the resulting trusted and revoked keys</summary>
    /// <param name="gamePath">Game folder whose .tempest/keys should be included</param>
    public async Task<int> KeyLists(string? gamePath = null)
    {
        var trust = await TrustStore.LoadAsync(gamePath == null ? null : GameFolderResolver.Resolve(gamePath));
        Console.WriteLine($"Key lists: {TrustStore.ListSourceDescription} ({(trust.OfflineReason == null ? "loaded" : $"unavailable: {trust.OfflineReason}; built-in keys only")})");
        Console.WriteLine("Trusted:");
        foreach (var key in trust.TrustedKeys.DistinctBy(ModSigning.Fingerprint))
        {
            var revoked = trust.IsRevoked(ModSigning.Fingerprint(key)) ? " (revoked)" : "";
            Console.WriteLine($"  {ModSigning.Describe(key)}{revoked}");
        }
        Console.WriteLine("Revoked:");
        foreach (var entry in trust.RevokedEntries.Distinct(StringComparer.OrdinalIgnoreCase))
            Console.WriteLine($"  {entry}");
        return trust.OfflineReason == null ? 0 : 1;
    }

    /// <summary>Signs trusted.keys and untrusted.keys for publishing, writing their .asc next to them</summary>
    /// <param name="dir">Folder holding the lists (the repo's keys/)</param>
    /// <param name="key">Private key to sign with; must be one of the list signers in TrustStore.cs</param>
    public async Task<int> SignKeyLists([Argument] string dir, string? key = null)
    {
        try
        {
            var secretKey = ModSigning.ReadSecretKey(await File.ReadAllTextAsync(key ?? TempestPathUtility.GetDefaultSigningKeyPath()));
            foreach (var name in new[] { TrustStore.TrustedListName, TrustStore.UntrustedListName })
            {
                TrustStore.SignList(Path.Combine(dir, name), secretKey);
                Console.WriteLine($"Signed {Path.Combine(dir, name)}.asc");
            }
            return 0;
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Failed to sign key lists: {ex.Message}");
            return 1;
        }
    }

    /// <summary>Signs a text with the mod signing key and prints the armored detached signature (for example the key-verification challenge from mods.lowrezstudio.com)</summary>
    /// <param name="text">The text to sign; `@path` signs the contents of a file instead</param>
    /// <param name="key">Private key file to sign with; defaults to $TEMPEST_SIGNING_KEY, then the one `mod gen-key` writes by default</param>
    public async Task<int> SignText([Argument] string text, string? key = null)
    {
        try
        {
            var secretKey = ModSigning.ReadSecretKey(await File.ReadAllTextAsync(key ?? TempestPathUtility.GetDefaultSigningKeyPath()));
            var content = text.StartsWith('@') ? await File.ReadAllTextAsync(text[1..]) : text;
            Console.Write(ModSigning.SignText(content, secretKey));
            return 0;
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Failed to sign: {ex.Message}");
            return 1;
        }
    }

    private static async Task<string> ReadEntryAsync(ZipArchiveEntry entry)
    {
        using var reader = new StreamReader(entry.Open());
        return await reader.ReadToEndAsync();
    }

    /// <summary>Prints the user id and fingerprint of a public or private key file</summary>
    /// <param name="keyFile">A .pub or .key file</param>
    public async Task<int> Fingerprint([Argument] string keyFile)
    {
        try
        {
            var armored = await File.ReadAllTextAsync(keyFile);
            var key = armored.Contains("PRIVATE KEY BLOCK") ? ModSigning.ReadSecretKey(armored).PublicKey : ModSigning.ReadPublicKey(armored);
            Console.WriteLine(ModSigning.Describe(key));
            return 0;
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Failed to read key: {ex.Message}");
            return 1;
        }
    }

    /// <summary>Generates an RSA keypair for signing mods and trusts the public key on this machine</summary>
    /// <param name="outputPath">Prefix path for output files (e.g. 'mykey' will create 'mykey.key' and 'mykey.pub'); defaults to the key `mod sign` uses</param>
    /// <param name="id">User id stored in the key, shown when a mod signed with it is verified</param>
    /// <param name="gamePath">Optional path to the game folder or executable to also trust the public key there</param>
    public int GenKey([Argument] string? outputPath = null, string id = "tempest@lowrez.studio", string? gamePath = null)
    {
        outputPath ??= Path.ChangeExtension(TempestPathUtility.GetDefaultSigningKeyPath(), null);
        PgpPublicKey publicKey;
        var pubKeyPath = $"{outputPath}.pub";
        try
        {
            publicKey = ModSigning.GenerateKeyPair(id, $"{outputPath}.key", pubKeyPath);
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Failed to generate keypair: {ex.Message}");
            return 1;
        }

        Console.WriteLine($"Generated private key: {outputPath}.key (keep it secret, it has no passphrase)");
        Console.WriteLine($"Generated public key: {pubKeyPath}");
        Console.WriteLine($"Key: {ModSigning.Describe(publicKey)}");

        // ponytail: auto-trust key by copying to global ~/.tempest/keys/, wine app data, and optionally the local game folder
        try
        {
            var globalKeysDir = TempestPathUtility.GetGlobalKeysDirectory();
            Directory.CreateDirectory(globalKeysDir);
            var destGlobalPubKeyPath = Path.Combine(globalKeysDir, Path.GetFileName(pubKeyPath));
            File.Copy(pubKeyPath, destGlobalPubKeyPath, overwrite: true);
            Console.WriteLine($"Automatically trusted key globally: {destGlobalPubKeyPath}");
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Warning: Could not save key to global trust store: {ex.Message}");
        }

        string? resolvedGame = null;
        try
        {
            resolvedGame = GameFolderResolver.Resolve(gamePath ?? Directory.GetCurrentDirectory());
        }
        catch (Exception ex)
        {
            if (gamePath != null) Console.Error.WriteLine($"Could not resolve specified game path: {ex.Message}");
        }

        if (resolvedGame != null)
        {
            try
            {
                var keysDir = TempestPathUtility.GetLocalKeysDirectory(resolvedGame);
                Directory.CreateDirectory(keysDir);
                var destPubKeyPath = Path.Combine(keysDir, Path.GetFileName(pubKeyPath));
                File.Copy(pubKeyPath, destPubKeyPath, overwrite: true);
                Console.WriteLine($"Automatically trusted key locally: {destPubKeyPath}");
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine($"Warning: Could not save key to local game trust store: {ex.Message}");
            }
        }
        return 0;
    }
}
