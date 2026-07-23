namespace Tempest.CLI.Mods;

public class ModV1Installer : IModInstaller
{
    public async Task<ModInstallResult> InstallAsync(string gamePath, string modFilePath, bool replace, bool allowUnsigned)
    {
        var resolvedGame = GameFolderResolver.Resolve(gamePath);
        var fileName = Path.GetFileName(modFilePath);

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

            // Backup existing file to .tempest/v1/backup
            var backupDir = TempestPathUtility.GetLocalV1BackupDirectory(resolvedGame);
            Directory.CreateDirectory(backupDir);
            var backupPath = TempestPathUtility.GetLocalV1BackupPath(resolvedGame, fileName);
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

        var modId = Guid.NewGuid().ToString();

        // Save snapshot for future re-enable
        var v1ModDir = TempestPathUtility.GetLocalV1ModDirectory(resolvedGame, modId);
        var snapshotDir = Path.Combine(v1ModDir, "files");
        Directory.CreateDirectory(snapshotDir);
        var snapshotPath = Path.Combine(snapshotDir, fileName);
        File.Copy(destPath, snapshotPath, overwrite: true);

        var modRecord = new ModRecord
        {
            Id = modId,
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
                    await Console.Error.WriteLineAsync($"Warning: Failed to patch DefaultEngine.ini: {ex.Message}");
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

    public async Task RemoveAsync(string gamePath, ModRecord mod)
    {
        var resolvedGame = GameFolderResolver.Resolve(gamePath);
        var backupDir = TempestPathUtility.GetLocalV1BackupDirectory(resolvedGame);

        await RemoveModFiles(resolvedGame, backupDir, mod);

        // Unregister in INI if non-asset mod
        await UnregisterIni(resolvedGame, mod);

        // Clean up snapshot and backup dirs
        var v1ModDir = TempestPathUtility.GetLocalV1ModDirectory(resolvedGame, mod.Id);
        try
        {
            if (Directory.Exists(v1ModDir)) Directory.Delete(v1ModDir, recursive: true);
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Warning: Failed to clean up mod directory: {ex.Message}");
        }

        foreach (var file in mod.InstalledFiles)
        {
            var fileName = Path.GetFileName(file);
            var backupPath = TempestPathUtility.GetLocalV1BackupPath(resolvedGame, fileName);
            try
            {
                if (File.Exists(backupPath)) File.Delete(backupPath);
            }
            catch (Exception ex)
            {
                await Console.Error.WriteLineAsync($"Warning: Failed to clean up backup {backupPath}: {ex.Message}");
            }
        }
    }

    public async Task DisableAsync(string gamePath, ModRecord mod)
    {
        var resolvedGame = GameFolderResolver.Resolve(gamePath);
        var backupDir = TempestPathUtility.GetLocalV1BackupDirectory(resolvedGame);

        // Copy backup back (keep backup intact for re-enable), delete mod file
        await DisableModFiles(resolvedGame, backupDir, mod);

        // Unregister INI entry
        await UnregisterIni(resolvedGame, mod);
    }

    public async Task EnableAsync(string gamePath, ModRecord mod)
    {
        var resolvedGame = GameFolderResolver.Resolve(gamePath);
        var v1ModDir = TempestPathUtility.GetLocalV1ModDirectory(resolvedGame, mod.Id);
        var snapshotDir = Path.Combine(v1ModDir, "files");

        foreach (var file in mod.InstalledFiles)
        {
            var fileName = Path.GetFileName(file);
            var snapshotPath = Path.Combine(snapshotDir, fileName);

            string? sourcePath = null;

            if (File.Exists(snapshotPath))
            {
                sourcePath = snapshotPath;
            }
            else if (File.Exists(mod.OriginalPath))
            {
                sourcePath = mod.OriginalPath;
            }

            if (sourcePath == null)
            {
                await Console.Error.WriteLineAsync($"Warning: Cannot enable mod '{mod.Name}': no snapshot or original file found. Try re-installing the mod.");
                continue;
            }

            try
            {
                var destDir = Path.GetDirectoryName(file);
                if (destDir != null) Directory.CreateDirectory(destDir);

                File.Copy(sourcePath, file, overwrite: true);
            }
            catch (Exception ex)
            {
                await Console.Error.WriteLineAsync($"Warning: Failed to enable file {file}: {ex.Message}");
            }
        }

        // Re-register in INI
        await RegisterIni(resolvedGame, mod);
    }

    private static async Task RemoveModFiles(string resolvedGame, string backupDir, ModRecord mod)
    {
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
    }

    private static async Task DisableModFiles(string resolvedGame, string backupDir, ModRecord mod)
    {
        foreach (var file in mod.InstalledFiles)
        {
            var fileName = Path.GetFileName(file);
            var backupPath = Path.Combine(backupDir, fileName);

            if (File.Exists(backupPath))
            {
                try
                {
                    var destDir = Path.GetDirectoryName(file);
                    if (destDir != null) Directory.CreateDirectory(destDir);

                    if (File.Exists(file)) File.Delete(file);
                    File.Copy(backupPath, file);
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
    }

    private static async Task UnregisterIni(string resolvedGame, ModRecord mod)
    {
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
    }

    private static async Task RegisterIni(string resolvedGame, ModRecord mod)
    {
        var shouldRegisterIni = !mod.Name.Contains("_SF", StringComparison.OrdinalIgnoreCase) &&
                                !mod.Name.Contains("WWB", StringComparison.OrdinalIgnoreCase);

        if (!shouldRegisterIni) return;

        var iniPath = Path.Combine(resolvedGame, "ChaosGame", "Config", "DefaultEngine.ini");

        if (!File.Exists(iniPath)) return;

        try
        {
            var lines = IniPatcher.Parse(iniPath);
            var packageName = Path.GetFileNameWithoutExtension(mod.Name);
            IniPatcher.AddNativePackage(lines, packageName);
            IniPatcher.Save(iniPath, lines);
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Warning: Failed to patch DefaultEngine.ini: {ex.Message}");
        }
    }
}
