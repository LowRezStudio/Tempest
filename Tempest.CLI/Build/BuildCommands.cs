using System.Reflection;
using System.Security.Cryptography;
using System.Text.Json;
using ConsoleAppFramework;

namespace Tempest.CLI.Build;

internal class BuildCommands
{
    /// <summary>Identifies the build using the manifest file</summary>
    /// <param name="path">Path to the game folder</param>
    /// <param name="json">Output as JSON</param>
    public async Task Identify([Argument] string path, bool json = false)
    {
        var assembly = Assembly.GetExecutingAssembly();
        var resourceName = "Tempest.CLI.Resources.manifests.csv";
        using var stream = assembly.GetManifestResourceStream(resourceName);

        if (stream == null)
        {
            Console.Error.WriteLine($"Manifest resource not found: {resourceName}");
            return;
        }

        using var reader = new StreamReader(stream);
        var content = await reader.ReadToEndAsync();
        var lines = content.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries);
        if (lines.Length <= 1) return;

        var header = lines[0].Split(',');
        var fileColumns = header.Skip(5).ToArray();

        var localHashes = new Dictionary<string, string>();
        foreach (var file in fileColumns)
        {
            var fullPath = Path.Combine(path, file);
            if (File.Exists(fullPath))
            {
                localHashes[file] = await GetFileHashAsync(fullPath);
            }
        }

        BuildInfo? bestMatch = null;
        int bestMatchCount = 0;
        int bestFileCount = 0;

        foreach (var line in lines.Skip(1))
        {
            var parts = line.Split(',');
            if (parts.Length < header.Length) continue;

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
                bestMatch = new BuildInfo
                {
                    Id = parts[0],
                    VersionGroup = parts[1],
                    PatchHotfix = bool.Parse(parts[2]),
                    PatchName = parts[3],
                    PatchWikiReference = $"https://paladins.fandom.com/wiki/{parts[4]}"
                };
            }
        }

        if (bestMatch == null)
        {
            Console.WriteLine("Build not identified.");
            return;
        }

        if (json)
        {
            Console.WriteLine(JsonSerializer.Serialize(bestMatch, BuildSourceGenerationContext.Default.BuildInfo));
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

    private static async Task<string> GetFileHashAsync(string filePath)
    {
        using var sha1 = SHA1.Create();
        using var stream = File.OpenRead(filePath);
        var hash = await sha1.ComputeHashAsync(stream);
        return Convert.ToHexStringLower(hash);
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
