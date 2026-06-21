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
            var mods = JsonSerializer.Deserialize(json, ModSourceGenerationContext.Default.ListModRecord) ?? [];
            var resolvedGame = GameFolderResolver.Resolve(gamePath);
            foreach (var mod in mods)
            {
                if (mod.InstalledFiles != null)
                {
                    for (int i = 0; i < mod.InstalledFiles.Count; i++)
                    {
                        var file = mod.InstalledFiles[i];
                        if (!Path.IsPathRooted(file))
                        {
                            mod.InstalledFiles[i] = Path.GetFullPath(Path.Combine(resolvedGame, file));
                        }
                    }
                }

                if (string.Equals(mod.Kind, "V2", StringComparison.OrdinalIgnoreCase))
                {
                    var modDir = Path.Combine(resolvedGame, ".tempest", "v2", "mods", mod.Id);
                    if (Directory.Exists(modDir))
                    {
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
                                if (File.Exists(Path.Combine(modDir, fallback)))
                                {
                                    foundReadmeFile = fallback;
                                    mod.Readme = fallback;
                                    break;
                                }
                            }
                        }

                        if (foundReadmeFile != null)
                        {
                            try
                            {
                                mod.ReadmeContent = File.ReadAllText(Path.Combine(modDir, foundReadmeFile));
                            }
                            catch
                            {
                                mod.ReadmeContent = "Error reading readme file.";
                            }
                        }
                    }
                }
            }
            return mods;
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
            var resolvedGame = GameFolderResolver.Resolve(gamePath);
            foreach (var mod in mods)
            {
                if (mod.InstalledFiles != null)
                {
                    for (int i = 0; i < mod.InstalledFiles.Count; i++)
                    {
                        var file = mod.InstalledFiles[i];
                        if (Path.IsPathRooted(file))
                        {
                            mod.InstalledFiles[i] = Path.GetRelativePath(resolvedGame, file);
                        }
                    }
                }
            }
            var json = JsonSerializer.Serialize(mods, ModSourceGenerationContext.Default.ListModRecord);
            File.WriteAllText(path, json);

            // ponytail: restore absolute paths in memory to avoid breaking subsequent operations/printing
            foreach (var mod in mods)
            {
                if (mod.InstalledFiles != null)
                {
                    for (int i = 0; i < mod.InstalledFiles.Count; i++)
                    {
                        var file = mod.InstalledFiles[i];
                        if (!Path.IsPathRooted(file))
                        {
                            mod.InstalledFiles[i] = Path.GetFullPath(Path.Combine(resolvedGame, file));
                        }
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
            if (string.Equals(mod.Kind, "V2", StringComparison.OrdinalIgnoreCase))
            {
                installer = new ModV2Installer();
            }
            else
            {
                try
                {
                    installer = ModInstallerFactory.CreateForFile(mod.Name);
                }
                catch
                {
                    installer = new ModV1Installer();
                }
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

    /// <summary>Renames a mod in the game instance</summary>
    /// <param name="path">Path to the game folder or executable</param>
    /// <param name="oldName">Old name of the mod</param>
    /// <param name="newName">New name of the mod</param>
    /// <param name="json">Output as JSON</param>
    public Task Rename([Argument] string path, [Argument] string oldName, [Argument] string newName, bool json = false)
    {
        try
        {
            var mods = LoadMetadata(path);
            var mod = mods.FirstOrDefault(m => string.Equals(m.Name, oldName, StringComparison.OrdinalIgnoreCase));

            if (mod == null)
            {
                var fail = new ModInstallResult { Success = false, Message = $"Mod not found: {oldName}" };
                PrintResult(fail, json);
                return Task.CompletedTask;
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
        return Task.CompletedTask;
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

    /// <summary>Packages a mod directory into a .tempest mod package, optionally signing it</summary>
    /// <param name="sourceDir">Path to the mod folder containing manifest.toml, files/, dlls/ etc</param>
    /// <param name="outputZip">Path to the output .tempest or .zip file</param>
    /// <param name="key">Optional path to a private key PEM file to sign the mod</param>
    public async Task Pack([Argument] string sourceDir, [Argument] string outputZip, string? key = null)
    {
        try
        {
            var fullSourceDir = Path.GetFullPath(sourceDir);
            if (!Directory.Exists(fullSourceDir))
            {
                Console.Error.WriteLine($"Error: Source directory does not exist: {sourceDir}");
                return;
            }

            var manifestPath = Path.Combine(fullSourceDir, "manifest.toml");
            if (!File.Exists(manifestPath))
            {
                Console.Error.WriteLine($"Error: manifest.toml not found in source directory: {sourceDir}");
                return;
            }

            var checksumEntries = new List<string>();
            var allFiles = Directory.GetFiles(fullSourceDir, "*", SearchOption.AllDirectories);
            Array.Sort(allFiles, StringComparer.Ordinal);

            foreach (var file in allFiles)
            {
                var relativePath = Path.GetRelativePath(fullSourceDir, file).Replace('\\', '/');
                if (relativePath == "CHECKSUMS" || relativePath == "CHECKSUMS.asc")
                    continue;

                var hash = ComputeSha256(file);
                checksumEntries.Add($"{hash} {relativePath}");
            }

            var checksumsContent = string.Join("\n", checksumEntries);
            var checksumsPath = Path.Combine(fullSourceDir, "CHECKSUMS");
            await File.WriteAllTextAsync(checksumsPath, checksumsContent);

            var ascPath = Path.Combine(fullSourceDir, "CHECKSUMS.asc");
            if (File.Exists(ascPath)) File.Delete(ascPath);

            if (!string.IsNullOrEmpty(key))
            {
                if (!File.Exists(key))
                {
                    Console.Error.WriteLine($"Error: Private key file not found: {key}");
                    return;
                }

                var privateKeyPem = await File.ReadAllTextAsync(key);
                var signedContent = ModV2Installer.SignData(checksumsContent, privateKeyPem);
                await File.WriteAllTextAsync(ascPath, signedContent);
                Console.WriteLine("Mod signed successfully.");
            }

            if (File.Exists(outputZip))
            {
                File.Delete(outputZip);
            }

            System.IO.Compression.ZipFile.CreateFromDirectory(fullSourceDir, outputZip);
            Console.WriteLine($"Successfully packed mod to '{outputZip}'");
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Failed to pack mod: {ex.Message}");
        }
    }

    /// <summary>Generates a new RSA private/public keypair for signing mods</summary>
    /// <param name="outputPath">Prefix path for output files (e.g. 'mykey' will create 'mykey.key' and 'mykey.pub')</param>
    public Task GenKey([Argument] string outputPath)
    {
        try
        {
            ModV2Installer.GeneratePgpKeyPair("tempest@lowrez.studio", $"{outputPath}.key", $"{outputPath}.pub");

            Console.WriteLine($"Generated private key: {outputPath}.key");
            Console.WriteLine($"Generated public key: {outputPath}.pub");
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Failed to generate keypair: {ex.Message}");
        }
        return Task.CompletedTask;
    }

    private static string ComputeSha256(string filePath)
    {
        using var sha256 = System.Security.Cryptography.SHA256.Create();
        using var stream = File.OpenRead(filePath);
        var hash = sha256.ComputeHash(stream);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }
}
