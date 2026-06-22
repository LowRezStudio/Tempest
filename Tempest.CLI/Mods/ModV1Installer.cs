namespace Tempest.CLI.Mods;

public class ModV1Installer : IModInstaller
{
    public async Task<ModInstallResult> InstallAsync(string gamePath, string modFilePath, bool replace, bool allowUnsigned)
    {
        if (!allowUnsigned)
        {
            return new ModInstallResult
            {
                Success = false,
                Unverified = true,
                Message = "Installing unsigned or unverified mods is not allowed without the '--allow-unsigned' flag."
            };
        }

        var resolvedGame = GameFolderResolver.Resolve(gamePath);
        var fileName = Path.GetFileName(modFilePath);
        _ = Path.GetExtension(modFilePath).ToLowerInvariant();

        // ponytail: simple name classification
        var isVoiceMod = fileName.Contains("_VOX", StringComparison.OrdinalIgnoreCase) ||
                         fileName.Contains("_VGS", StringComparison.OrdinalIgnoreCase);
        var isAssetMod = fileName.Contains("_SF", StringComparison.OrdinalIgnoreCase) ||
                         fileName.Contains("_PC", StringComparison.OrdinalIgnoreCase);

        var kind = "NativePackage";
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
                var mods = ModCommands.LoadMetadata(gamePath);
                var isMod = mods.Any(m => string.Equals(m.Name, fileName, StringComparison.OrdinalIgnoreCase) ||
                                          m.InstalledFiles.Any(f => string.Equals(f, destPath, StringComparison.OrdinalIgnoreCase)));

                return new ModInstallResult
                {
                    Success = false,
                    Conflict = true,
                    IsModConflict = isMod,
                    Message = $"Mod with filename '{fileName}' already exists in destination."
                };
            }

            // Backup existing file to ChaosGame/CookedAssetBackup
            var backupDir = Path.Combine(resolvedGame, "ChaosGame", "CookedAssetBackup");
            Directory.CreateDirectory(backupDir);
            var backupPath = Path.Combine(backupDir, fileName);
            try
            {
                if (File.Exists(backupPath))
                {
                    // If a file already exists in backup, it's considered priority (the pristine original)
                    // and we do NOT make a new backup of the already-modified destPath.
                    // Instead, we just delete the existing destPath before copying the new mod file.
                    File.Delete(destPath);
                }
                else
                {
                    File.Move(destPath, backupPath);
                }
            }
            catch (Exception ex)
            {
                // Non-fatal if backup/deletion fails
                await Console.Error.WriteLineAsync($"Warning: Failed to handle backup or deletion of existing file: {ex.Message}");
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
        var shouldRegisterIni = !fileName.Contains("_SF", StringComparison.OrdinalIgnoreCase) &&
                                !fileName.Contains("WWB", StringComparison.OrdinalIgnoreCase);

        if (!shouldRegisterIni)
            return new ModInstallResult
            {
                Success = true,
                Message = "Mod installed successfully",
                Mod = modRecord
            };
        {
            var iniPath = Path.Combine(resolvedGame, "ChaosGame", "Config", "DefaultEngine.ini");
            
            if (!File.Exists(iniPath))
                return new ModInstallResult
                {
                    Success = true,
                    Message = "Mod installed successfully",
                    Mod = modRecord
                };
            
            try
            {
                var lines = IniPatcher.Parse(iniPath);
                var packageName = Path.GetFileNameWithoutExtension(fileName);
                IniPatcher.AddNativePackage(lines, packageName);
                IniPatcher.Save(iniPath, lines);
            }
            catch (Exception ex)
            {
                await Console.Error.WriteLineAsync($"Warning: Failed to patch DefaultEngine.ini: {ex.Message}");
            }
        }

        return new ModInstallResult
        {
            Success = true,
            Message = "Mod installed successfully",
            Mod = modRecord
        };
    }

    public async Task RemoveAsync(string gamePath, ModRecord mod)
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
                    await Console.Error.WriteLineAsync($"Warning: Failed to restore backup file {backupPath} to {file}: {ex.Message}");
                }
            }
            else
            {
                if (!File.Exists(file)) continue;
                
                try
                {
                    File.Delete(file);
                }
                catch (Exception ex)
                {
                    await Console.Error.WriteLineAsync($"Warning: Failed to delete installed file {file}: {ex.Message}");
                }
            }
        }

        // Unregister in INI if non-asset mod
        var shouldUnregisterIni = !mod.Name.Contains("_SF", StringComparison.OrdinalIgnoreCase) &&
                                  !mod.Name.Contains("WWB", StringComparison.OrdinalIgnoreCase);

        if (!shouldUnregisterIni) return;
        
        var iniPath = Path.Combine(resolvedGame, "ChaosGame", "Config", "DefaultEngine.ini");
        
        if (!File.Exists(iniPath)) return;
        
        try
        {
            var lines = IniPatcher.Parse(iniPath);
            var packageName = Path.GetFileNameWithoutExtension(mod.Name);
            IniPatcher.RemoveNativePackage(lines, packageName);
            IniPatcher.Save(iniPath, lines);
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Warning: Failed to unpatch DefaultEngine.ini: {ex.Message}");
        }

        return;
    }
}
