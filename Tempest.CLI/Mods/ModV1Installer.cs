namespace Tempest.CLI.Mods;

public class ModV1Installer : IModInstaller
{
    public async Task<ModInstallResult> InstallAsync(string gamePath, string modFilePath, bool replace, bool stack, bool allowUnsigned)
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
        var newRelative = Path.GetRelativePath(resolvedGame, destPath).Replace('\\', '/');

        // Component overlap detection: check if any installed mod already edits this file
        // Use OwnedFiles (current ownership) not InstalledFiles (historical)
        // Compare using relative paths for cross-format compatibility (V1 vs V2)
        var metadataMods = ModCommands.LoadMetadata(gamePath);
        var conflicts = new List<ModConflictInfo>();
        var destRelative = Path.GetRelativePath(resolvedGame, destPath).Replace('\\', '/');

        foreach (var existingMod in metadataMods)
        {
            var overlapping = existingMod.OwnedFiles
                .Where(f => {
                    var ext = Path.GetExtension(f).ToLowerInvariant();
                    if (ext == ".ini") return false;
                    try
                    {
                        var rel = Path.GetRelativePath(resolvedGame, f).Replace('\\', '/');
                        return string.Equals(rel, destRelative, StringComparison.OrdinalIgnoreCase);
                    }
                    catch { return false; }
                })
                .Select(f => {
                    try { return Path.GetRelativePath(resolvedGame, f).Replace('\\', '/'); }
                    catch { return f; }
                })
                .ToList();
            if (overlapping.Count > 0)
            {
                conflicts.Add(new ModConflictInfo
                {
                    ModId = existingMod.Id,
                    ModName = existingMod.Name,
                    ModVersion = existingMod.Version,
                    ConflictingFiles = overlapping
                });
            }
            else if (string.Equals(existingMod.Name, fileName, StringComparison.OrdinalIgnoreCase))
            {
                // fallback: same filename mod (V1 name collision) even if OwnedFiles not matched due to path normalization
                conflicts.Add(new ModConflictInfo
                {
                    ModId = existingMod.Id,
                    ModName = existingMod.Name,
                    ModVersion = existingMod.Version,
                    ConflictingFiles = [newRelative]
                });
            }
        }

        if (conflicts.Count > 0)
        {
            if (!replace && !stack)
            {
                var names = string.Join(", ", conflicts.Select(c => $"'{c.ModName}'"));
                var files = string.Join(", ", conflicts.SelectMany(c => c.ConflictingFiles).Distinct(StringComparer.OrdinalIgnoreCase));
                return new ModInstallResult
                {
                    Success = false,
                    Conflict = true,
                    IsModConflict = true,
                    Message = $"Mod '{fileName}' conflicts with {names} (overlapping files: {files}).",
                    ConflictingMods = conflicts,
                    NewModName = fileName
                };
            }

            if (replace)
            {
                // replace requested — remove all conflicting mods before installing
                var allMods = ModCommands.LoadMetadata(gamePath);
                foreach (var c in conflicts)
                {
                    var modToRemove = allMods.FirstOrDefault(m => string.Equals(m.Id, c.ModId, StringComparison.OrdinalIgnoreCase));
                    if (modToRemove != null)
                    {
                        var installer = ModCommands.CreateInstaller(modToRemove);
                        await installer.RemoveAsync(gamePath, modToRemove);
                        allMods.Remove(modToRemove);
                    }
                }
                ModCommands.SaveMetadata(gamePath, allMods);
            }
            else if (stack)
            {
                // stack requested — transfer ownership of conflicting files to new mod
                var allMods = ModCommands.LoadMetadata(gamePath);
                var conflictingFileSet = new HashSet<string>(conflicts.SelectMany(c => c.ConflictingFiles), StringComparer.OrdinalIgnoreCase);

                foreach (var existingMod in allMods)
                {
                    // Remove conflicting files from existing mod's OwnedFiles
                    existingMod.OwnedFiles.RemoveAll(f => conflictingFileSet.Contains(Path.GetRelativePath(resolvedGame, f).Replace('\\', '/')));
                }
                ModCommands.SaveMetadata(gamePath, allMods);
            }
        }

        // Backup handling: preserve pristine original before overwriting (vanilla or previously restored)
        if (File.Exists(destPath))
        {
            var backupDir = TempestPathUtility.GetLocalV1BackupDirectory(resolvedGame);
            Directory.CreateDirectory(backupDir);
            var backupPath = TempestPathUtility.GetLocalV1BackupPath(resolvedGame, fileName);
            try
            {
                if (File.Exists(backupPath))
                {
                    // Backup already exists (pristine original), do not overwrite.
                    File.Delete(destPath);
                }
                else
                {
                    // First mod touching this file - preserve current as backup.
                    File.Move(destPath, backupPath);
                }
            }
            catch (Exception ex)
            {
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
            OriginalPath = modFilePath,
            InstalledFiles = [destPath],
            OwnedFiles = [destPath],
            MetadataVersion = 2
        };

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

        foreach (var file in mod.OwnedFiles)
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
        foreach (var file in mod.OwnedFiles)
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
        foreach (var file in mod.OwnedFiles)
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
