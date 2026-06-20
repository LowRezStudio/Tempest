using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Threading.Tasks;
using ConsoleAppFramework;

namespace Tempest.CLI.Mods;

internal class ModCommands
{
    internal static string GetMetadataPath(string gamePath)
    {
        var resolvedGame = GameFolderResolver.Resolve(gamePath);
        var metadataDir = Path.Combine(resolvedGame, ".tempest", "mods");
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
            return JsonSerializer.Deserialize(json, ModSourceGenerationContext.Default.ListModRecord) ?? [];
        }
        catch
        {
            return [];
        }
    }

    private static void SaveMetadata(string gamePath, List<ModRecord> mods)
    {
        var path = GetMetadataPath(gamePath);
        try
        {
            var json = JsonSerializer.Serialize(mods, ModSourceGenerationContext.Default.ListModRecord);
            File.WriteAllText(path, json);
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
    /// <param name="json">Output as JSON</param>
    public async Task Install([Argument] string path, [Argument] string modFile, bool replace = false, bool json = false)
    {
        try
        {
            if (!File.Exists(modFile))
            {
                var fail = new ModInstallResult { Success = false, Message = $"Mod file not found: {modFile}" };
                PrintResult(fail, json);
                return;
            }

            var installer = ModInstallerFactory.CreateForFile(modFile);
            var result = await installer.InstallAsync(path, modFile, replace);

            if (result.Success && result.Mod != null)
            {
                var mods = LoadMetadata(path);
                mods.RemoveAll(m => string.Equals(m.Name, result.Mod.Name, StringComparison.OrdinalIgnoreCase));
                mods.Add(result.Mod);
                SaveMetadata(path, mods);
            }

            PrintResult(result, json);
        }
        catch (Exception ex)
        {
            var fail = new ModInstallResult { Success = false, Message = ex.Message };
            PrintResult(fail, json);
        }
    }

    /// <summary>Lists installed mods for the game instance</summary>
    /// <param name="path">Path to the game folder or executable</param>
    /// <param name="json">Output as JSON</param>
    public Task List([Argument] string path, bool json = false)
    {
        try
        {
            var mods = LoadMetadata(path);
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
        return Task.CompletedTask;
    }

    /// <summary>Removes a mod from the game instance</summary>
    /// <param name="path">Path to the game folder or executable</param>
    /// <param name="modName">Name of the mod to remove (e.g. TgMod.upk)</param>
    /// <param name="json">Output as JSON</param>
    public async Task Remove([Argument] string path, [Argument] string modName, bool json = false)
    {
        try
        {
            var mods = LoadMetadata(path);
            var mod = mods.FirstOrDefault(m => string.Equals(m.Name, modName, StringComparison.OrdinalIgnoreCase));

            if (mod == null)
            {
                var fail = new ModInstallResult { Success = false, Message = $"Mod not found: {modName}" };
                PrintResult(fail, json);
                return;
            }

            IModInstaller installer;
            try
            {
                installer = ModInstallerFactory.CreateForFile(mod.Name);
            }
            catch
            {
                installer = new ModV1Installer();
            }

            await installer.RemoveAsync(path, mod);
            mods.Remove(mod);
            SaveMetadata(path, mods);

            var ok = new ModInstallResult { Success = true, Message = $"Mod '{modName}' removed successfully." };
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
    /// <param name="json">Output as JSON</param>
    public async Task InstallBulk([Argument] string path, string[] modFiles, bool replace = false, bool json = false)
    {
        var results = new List<ModInstallResult>();
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
                var result = await installer.InstallAsync(path, file, replace);

                if (result.Success && result.Mod != null)
                {
                    var mods = LoadMetadata(path);
                    mods.RemoveAll(m => string.Equals(m.Name, result.Mod.Name, StringComparison.OrdinalIgnoreCase));
                    mods.Add(result.Mod);
                    SaveMetadata(path, mods);
                }
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
}
