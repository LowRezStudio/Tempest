using System;
using System.IO;
using System.Threading.Tasks;

namespace Tempest.CLI.Mods;

public class ModV1Installer : IModInstaller
{
    public async Task<ModInstallResult> InstallAsync(string gamePath, string modFilePath, bool replace)
    {
        var resolvedGame = GameFolderResolver.Resolve(gamePath);
        var fileName = Path.GetFileName(modFilePath);
        _ = Path.GetExtension(modFilePath).ToLowerInvariant();

        // ponytail: simple name classification
        bool isVoiceMod = fileName.Contains("_VOX", StringComparison.OrdinalIgnoreCase) ||
                          fileName.Contains("_VGS", StringComparison.OrdinalIgnoreCase);
        bool isAssetMod = fileName.Contains("_SF", StringComparison.OrdinalIgnoreCase) ||
                          fileName.Contains("_PC", StringComparison.OrdinalIgnoreCase);

        string kind = "NativePackage";
        if (isVoiceMod) kind = "Voice";
        else if (isAssetMod) kind = "Asset";

        var destDir = isVoiceMod
            ? Path.Combine(resolvedGame, "ChaosGame", "CookedPCConsole", "English(US)")
            : Path.Combine(resolvedGame, "ChaosGame", "CookedPCConsole");

        Directory.CreateDirectory(destDir);
        var destPath = Path.Combine(destDir, fileName);

        if (File.Exists(destPath))
        {
            if (!replace)
            {
                return new ModInstallResult
                {
                    Success = false,
                    Conflict = true,
                    Message = $"Mod with filename '{fileName}' already exists in destination."
                };
            }

            // Backup existing file to ChaosGame/CookedAssetBackup
            var backupDir = Path.Combine(resolvedGame, "ChaosGame", "CookedAssetBackup");
            Directory.CreateDirectory(backupDir);
            var backupPath = Path.Combine(backupDir, fileName);
            try
            {
                if (File.Exists(backupPath)) File.Delete(backupPath);
                File.Move(destPath, backupPath);
            }
            catch (Exception ex)
            {
                // Non-fatal if backup fails
                Console.Error.WriteLine($"Warning: Failed to backup existing file: {ex.Message}");
            }
        }

        // Copy mod file
        File.Copy(modFilePath, destPath, overwrite: true);

        var modRecord = new ModRecord
        {
            Id = Guid.NewGuid().ToString(),
            Name = fileName,
            Author = "Unknown",
            Version = string.Empty,
            Enabled = true,
            Kind = kind,
            OriginalPath = modFilePath
        };
        modRecord.InstalledFiles.Add(destPath);

        // Register in INI if non-asset mod
        bool shouldRegisterIni = !fileName.Contains("_SF", StringComparison.OrdinalIgnoreCase) &&
                                 !fileName.Contains("WWB", StringComparison.OrdinalIgnoreCase);

        if (shouldRegisterIni)
        {
            var iniPath = Path.Combine(resolvedGame, "ChaosGame", "Config", "DefaultEngine.ini");
            if (File.Exists(iniPath))
            {
                try
                {
                    var lines = IniPatcher.Parse(iniPath);
                    var packageName = Path.GetFileNameWithoutExtension(fileName);
                    IniPatcher.AddNativePackage(lines, packageName);
                    IniPatcher.Save(iniPath, lines);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine($"Warning: Failed to patch DefaultEngine.ini: {ex.Message}");
                }
            }
        }

        return new ModInstallResult
        {
            Success = true,
            Message = "Mod installed successfully",
            Mod = modRecord
        };
    }

    public Task RemoveAsync(string gamePath, ModRecord mod)
    {
        var resolvedGame = GameFolderResolver.Resolve(gamePath);
        var backupDir = Path.Combine(resolvedGame, "ChaosGame", "CookedAssetBackup");

        // Delete copied files and restore backups if available
        foreach (var file in mod.InstalledFiles)
        {
            var fileName = Path.GetFileName(file);
            var backupPath = Path.Combine(backupDir, fileName);

            if (File.Exists(backupPath))
            {
                try
                {
                    if (File.Exists(file)) File.Delete(file);
                    File.Move(backupPath, file);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine($"Warning: Failed to restore backup file {backupPath} to {file}: {ex.Message}");
                }
            }
            else
            {
                if (File.Exists(file))
                {
                    try
                    {
                        File.Delete(file);
                    }
                    catch (Exception ex)
                    {
                        Console.Error.WriteLine($"Warning: Failed to delete installed file {file}: {ex.Message}");
                    }
                }
            }
        }

        // Unregister in INI if non-asset mod
        bool shouldUnregisterIni = !mod.Name.Contains("_SF", StringComparison.OrdinalIgnoreCase) &&
                                   !mod.Name.Contains("WWB", StringComparison.OrdinalIgnoreCase);

        if (shouldUnregisterIni)
        {
            var iniPath = Path.Combine(resolvedGame, "ChaosGame", "Config", "DefaultEngine.ini");
            if (File.Exists(iniPath))
            {
                try
                {
                    var lines = IniPatcher.Parse(iniPath);
                    var packageName = Path.GetFileNameWithoutExtension(mod.Name);
                    IniPatcher.RemoveNativePackage(lines, packageName);
                    IniPatcher.Save(iniPath, lines);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine($"Warning: Failed to unpatch DefaultEngine.ini: {ex.Message}");
                }
            }
        }

        return Task.CompletedTask;
    }
}
