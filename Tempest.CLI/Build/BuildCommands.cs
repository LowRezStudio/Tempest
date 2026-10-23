using System.Collections.Concurrent;
using System.Reflection;
using System.Security.Cryptography;
using System.Text.Json;
using ConsoleAppFramework;

namespace Tempest.CLI.Build;

internal class BuildCommands
{
    private const string ManifestResource = "Tempest.CLI.Resources.manifests.csv";

    private static readonly string[] GameMarkers =
    [
        "Binaries/Win64/Paladins.exe",
        "Binaries/Win32/Paladins.exe",
        "ChaosGame/CookedPCConsole",
    ];

    /// <summary>Identifies the build using the manifest file</summary>
    /// <param name="path">Path to the game folder</param>
    /// <param name="json">Output as JSON</param>
    public async Task Identify([Argument] string path, bool json = false)
    {
        var assembly = Assembly.GetExecutingAssembly();
        using var stream = assembly.GetManifestResourceStream(ManifestResource);

        if (stream == null)
        {
            Console.Error.WriteLine($"Manifest resource not found: {ManifestResource}");
            Environment.ExitCode = 1;
            return;
        }

        using var reader = new StreamReader(stream);
        var content = await reader.ReadToEndAsync();
        var lines = content.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries);
        if (lines.Length <= 1)
        {
            Console.Error.WriteLine("Manifest resource is empty.");
            Environment.ExitCode = 1;
            return;
        }

        var header = lines[0].Split(',');
        var fileColumns = header.Skip(5).ToArray();
        var rows = lines.Skip(1).Select(l => l.Split(',')).Where(p => p.Length >= header.Length).ToArray();

        // The files are independent, and a real install's TgClient.upk alone is over a gigabyte, so hash them in parallel.
        var localHashes = new ConcurrentDictionary<string, string>();
        await Parallel.ForEachAsync(fileColumns, (file, _) =>
        {
            var fullPath = ResolvePath(path, file);
            if (fullPath == null || !File.Exists(fullPath)) return ValueTask.CompletedTask;
            try
            {
                localHashes[file] = GetFileHash(fullPath);
            }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException)
            {
                Console.Error.WriteLine($"Could not read {fullPath}: {e.Message}");
            }
            return ValueTask.CompletedTask;
        });

        BuildInfo? bestMatch = null;
        int bestMatchCount = 0;
        int bestFileCount = 0;

        foreach (var parts in rows)
        {
            int matchCount = 0;
            int fileCount = 0;
            for (int i = 5; i < header.Length; i++)
            {
                var expectedHash = parts[i];
                if (string.IsNullOrEmpty(expectedHash)) continue;

                var fileName = header[i];
                fileCount++;
                if (localHashes.TryGetValue(fileName, out var actualHash) && actualHash == expectedHash)
                {
                    matchCount++;
                }
            }

            if (fileCount == 0) continue;

            if (matchCount > bestMatchCount || (matchCount == bestMatchCount && fileCount > bestFileCount))
            {
                bestMatchCount = matchCount;
                bestFileCount = fileCount;
                bestMatch = ToBuildInfo(parts);
            }
        }

        if (bestMatchCount == 0)
        {
            bestMatch = null;
        }

        // Patched or modded files no longer hash-match, but the game still reports its own version.
        var gameVersion = ReadGameVersion(path);
        if (bestMatch == null && gameVersion != null)
        {
            var row = rows.FirstOrDefault(p => p[4].EndsWith($"_{gameVersion}", StringComparison.Ordinal));
            if (row != null) bestMatch = ToBuildInfo(row);
        }

        if (bestMatch == null && GameMarkers.Any(m => ResolvePath(path, m) != null))
        {
            bestMatch = new BuildInfo
            {
                VersionGroup = gameVersion == null ? string.Empty : string.Join('.', gameVersion.Split('.').Take(2)),
                PatchName = gameVersion ?? string.Empty,
            };
        }

        if (bestMatch == null)
        {
            Console.Error.WriteLine("Not a Paladins installation: no game files found.");
            Environment.ExitCode = 1;
            return;
        }

        if (json)
        {
            Console.WriteLine(JsonSerializer.Serialize(bestMatch, BuildSourceGenerationContext.Default.BuildInfo));
        }
        else if (string.IsNullOrEmpty(bestMatch.Id))
        {
            Console.WriteLine("Unknown build (Paladins installation not in the build list)");
            if (gameVersion != null) Console.WriteLine($"  Version: {gameVersion}");
        }
        else
        {
            var confidence = bestFileCount == 0 ? 0 : (int)Math.Round((double)bestMatchCount / bestFileCount * 100);
            Console.WriteLine($"Build Identified (closest match):");
            Console.WriteLine($"  ID:      {bestMatch.Id}");
            Console.WriteLine($"  Version: {bestMatch.VersionGroup}");
            Console.WriteLine($"  Patch:   {bestMatch.PatchName}");
            Console.WriteLine($"  Hotfix:  {bestMatch.PatchHotfix}");
            Console.WriteLine($"  Wiki:    {bestMatch.PatchWikiReference}");
            Console.WriteLine($"  Match:   {bestMatchCount}/{bestFileCount} ({confidence}%)");
        }
    }

    private static BuildInfo ToBuildInfo(string[] parts) => new()
    {
        Id = parts[0],
        VersionGroup = parts[1],
        PatchHotfix = bool.Parse(parts[2]),
        PatchName = parts[3],
        PatchWikiReference = $"https://paladins.fandom.com/wiki/{parts[4]}"
    };

    private static string? ReadGameVersion(string gamePath)
    {
        var file = ResolvePath(gamePath, "Binaries/GameVersion.txt");
        if (file == null) return null;
        try
        {
            var text = File.ReadAllText(file).Trim();
            return Version.TryParse(text, out _) ? text : null;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return null;
        }
    }

    /// <summary>Resolves a relative path ignoring case, for case-sensitive file systems and copies that changed the depot's casing.</summary>
    private static string? ResolvePath(string root, string relativePath)
    {
        var exact = Path.Combine(root, relativePath);
        if (Path.Exists(exact)) return exact;

        try
        {
            var current = root;
            foreach (var segment in relativePath.Split('/', '\\'))
            {
                if (!Directory.Exists(current)) return null;
                var match = Directory.EnumerateFileSystemEntries(current)
                    .FirstOrDefault(e => string.Equals(Path.GetFileName(e), segment, StringComparison.OrdinalIgnoreCase));
                if (match == null) return null;
                current = match;
            }
            return current;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return null;
        }
    }

    private static string GetFileHash(string filePath)
    {
        using var stream = File.OpenRead(filePath);
        return Convert.ToHexStringLower(SHA1.HashData(stream));
    }
}

internal class BuildInfo
{
    public string Id { get; set; } = string.Empty;
    public string VersionGroup { get; set; } = string.Empty;
    public bool PatchHotfix { get; set; }
    public string PatchName { get; set; } = string.Empty;
    public string PatchWikiReference { get; set; } = string.Empty;
}
