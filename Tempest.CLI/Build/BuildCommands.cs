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

        foreach (var line in lines.Skip(1))
        {
            var parts = line.Split(',');
            if (parts.Length < header.Length) continue;

            bool matches = true;
            bool anyFileChecked = false;
            for (int i = 5; i < header.Length; i++)
            {
                var expectedHash = parts[i];
                if (string.IsNullOrEmpty(expectedHash)) continue;

                var fileName = header[i];
                anyFileChecked = true;
                if (!localHashes.TryGetValue(fileName, out var actualHash) || actualHash != expectedHash)
                {
                    matches = false;
                    break;
                }
            }

            if (matches && anyFileChecked)
            {
                var result = new BuildInfo
                {
                    Id = parts[0],
                    VersionGroup = parts[1],
                    PatchHotfix = bool.Parse(parts[2]),
                    PatchName = parts[3],
                    PatchWikiReference = $"https://paladins.fandom.com/wiki/{parts[4]}"
                };

                if (json)
                {
                    Console.WriteLine(JsonSerializer.Serialize(result, BuildSourceGenerationContext.Default.BuildInfo));
                }
                else
                {
                    Console.WriteLine($"Build Identified:");
                    Console.WriteLine($"  ID:      {result.Id}");
                    Console.WriteLine($"  Version: {result.VersionGroup}");
                    Console.WriteLine($"  Patch:   {result.PatchName}");
                    Console.WriteLine($"  Hotfix:  {result.PatchHotfix}");
                    Console.WriteLine($"  Wiki:    {result.PatchWikiReference}");
                }
                return;
            }
        }

        Console.WriteLine("Build not identified.");
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
