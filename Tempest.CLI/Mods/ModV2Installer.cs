using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using Tomlyn;
using Tomlyn.Model;
using Tomlyn.Serialization;

namespace Tempest.CLI.Mods;

[TomlSerializable(typeof(TomlTable))]
internal partial class ModTomlContext : TomlSerializerContext
{
}

public class ModV2Installer : IModInstaller
{
    private static string NormalizeEntryPath(string fullName) => fullName.Replace('\\', '/');
    internal static bool IsDirectoryEntry(ZipArchiveEntry entry) =>
        entry.FullName.EndsWith('/') || string.IsNullOrEmpty(entry.Name);

    /// <summary>
    /// Rejects packages that would write outside their mod folder or the game folder, or into
    /// <c>&lt;game&gt;/.tempest</c> (trusted keys, mod metadata and other mods' DLLs live there).
    /// </summary>
    private static void ValidateLayout(ZipArchive archive, string resolvedGame, string modId)
    {
        if (modId is "." or ".." || modId.IndexOfAny(['/', '\\', ':']) >= 0 || modId.IndexOfAny(Path.GetInvalidFileNameChars()) >= 0)
            throw new Exception($"Invalid mod id '{modId}'.");

        var tempestDir = Path.GetFullPath(Path.Combine(resolvedGame, ".tempest"));
        var targetModDir = TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, modId);
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var entry in archive.Entries)
        {
            if (IsDirectoryEntry(entry)) continue;

            var relativePath = NormalizeEntryPath(entry.FullName);
            if (!seen.Add(relativePath))
                throw new Exception($"Mod contains '{relativePath}' more than once.");

            if (relativePath.StartsWith("files/", StringComparison.OrdinalIgnoreCase))
            {
                var dest = ResolveUnder(resolvedGame, relativePath["files/".Length..]);
                if (IsSameOrBelow(dest, tempestDir, StringComparison.OrdinalIgnoreCase))
                    throw new Exception($"'{relativePath}' would write into the game's .tempest folder.");
            }
            else
            {
                ResolveUnder(targetModDir, relativePath);
            }
        }
    }

    private static string ResolveUnder(string root, string relativePath)
    {
        var fullRoot = Path.GetFullPath(root);
        var full = Path.GetFullPath(Path.Combine(fullRoot, relativePath));
        if (!IsSameOrBelow(full, fullRoot, StringComparison.Ordinal))
            throw new Exception($"'{relativePath}' points outside of '{fullRoot}'.");
        return full;
    }

    private static bool IsSameOrBelow(string path, string root, StringComparison comparison) =>
        path.Equals(root, comparison) ||
        path.StartsWith(Path.EndsInDirectorySeparator(root) ? root : root + Path.DirectorySeparatorChar, comparison);

    /// <summary>
    /// Entries that can run code: PE images (an "MZ" header, whatever the extension) and Lua scripts
    /// (<c>.lua</c>, except the <c>.d.lua</c> editor stubs the runtime never loads).
    /// </summary>
    private static async Task<List<string>> FindCodeAsync(ZipArchive archive)
    {
        var found = new List<string>();
        var header = new byte[2];
        foreach (var entry in archive.Entries)
        {
            if (IsDirectoryEntry(entry)) continue;
            var path = NormalizeEntryPath(entry.FullName);
            if (path.EndsWith(".lua", StringComparison.OrdinalIgnoreCase) && !path.EndsWith(".d.lua", StringComparison.OrdinalIgnoreCase))
            {
                found.Add(path);
                continue;
            }
            await using var stream = await entry.OpenAsync();
            if (await stream.ReadAtLeastAsync(header, 2, throwOnEndOfStream: false) == 2 && header is [(byte)'M', (byte)'Z'])
                found.Add(path);
        }
        return found;
    }

    private static async Task<string> ReadEntryAsync(ZipArchiveEntry entry)
    {
        using var reader = new StreamReader(await entry.OpenAsync(), Encoding.UTF8);
        return await reader.ReadToEndAsync();
    }

    public async Task<ModInstallResult> InstallAsync(string gamePath, string modFilePath, bool replace, bool stack, bool allowUntrusted)
    {
        var resolvedGame = GameFolderResolver.Resolve(gamePath);

        try
        {
            await using var fileStream = File.OpenRead(modFilePath);
            await using var archive = new ZipArchive(fileStream, ZipArchiveMode.Read);

            var manifestEntry = archive.GetEntry("manifest.toml");
            if (manifestEntry == null)
            {
                throw new Exception("Mod does not contain manifest.toml");
            }

            string manifestContent;
            using (var reader = new StreamReader(await manifestEntry.OpenAsync(), Encoding.UTF8))
            {
                manifestContent = await reader.ReadToEndAsync();
            }

            var manifest = ParseManifest(manifestContent);
            if (string.IsNullOrEmpty(manifest.Id))
            {
                throw new Exception("manifest.toml does not contain a valid mod id");
            }

            var modId = manifest.Id;
            ValidateLayout(archive, resolvedGame, modId);

            // Mods without code (no DLL, no Lua) cannot do harm and install whoever signed them.
            // With code: a trusted signature installs, an untrusted one needs allowUntrusted (the
            // launcher asks first), anything else is refused.
            var code = await FindCodeAsync(archive);
            var trust = code.Count > 0 ? await TrustStore.LoadAsync(resolvedGame) : null;

            var checksumsEntry = archive.GetEntry(ModSigning.ChecksumsName);
            var verification = new ModSigning.Verification(ModSigning.Trust.Unsigned, null, null);
            if (checksumsEntry != null)
            {
                var checksumsContent = await ReadEntryAsync(checksumsEntry);
                await VerifyArchiveChecksumsAsync(archive, ParseChecksumMap(checksumsContent));
                if (trust != null)
                {
                    var asc = archive.GetEntry(ModSigning.SignatureName) is { } ascEntry ? await ReadEntryAsync(ascEntry) : null;
                    var signerKey = archive.GetEntry(ModSigning.SignerKeyName) is { } keyEntry ? await ReadEntryAsync(keyEntry) : null;
                    verification = ModSigning.Verify(checksumsContent, asc, signerKey, trust);
                }
            }

            if (trust != null && !(verification.Trust == ModSigning.Trust.Trusted || verification.Trust == ModSigning.Trust.Untrusted && allowUntrusted))
            {
                var contents = $"'{manifest.Name}' contains code ({string.Join(", ", code.Take(3))}{(code.Count > 3 ? ", ..." : "")})";
                var keyListNote = trust.OfflineReason == null ? "" : " The key lists could not be reached, so only the built-in keys are trusted for now.";
                return new ModInstallResult
                {
                    Success = false,
                    Unverified = verification.Trust == ModSigning.Trust.Untrusted,
                    Signer = verification.Trust == ModSigning.Trust.Untrusted ? verification.Signer : null,
                    KeyListOffline = trust.OfflineReason != null,
                    Message = verification.Trust switch
                    {
                        ModSigning.Trust.Untrusted => $"{contents} signed by {verification.Signer}, which is not a trusted key. It could be harmful; pass --allow-untrusted to install it anyway.{keyListNote}",
                        ModSigning.Trust.Revoked => $"{contents} signed by {verification.Signer}, a key that has been revoked.",
                        ModSigning.Trust.Invalid => $"{contents} and an invalid signature: {verification.Problem}.",
                        _ => $"{contents} but is not signed. Mods with DLLs or Lua scripts must be signed (tempest mod sign).",
                    }
                };
            }

            var targetModDir = TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, modId);

            // Collect new mod's component files (INI exempt) for overlap detection
            var newRelativeFiles = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            var newRelativeFilesList = new List<string>();
            foreach (var entry in archive.Entries)
            {
                if (IsDirectoryEntry(entry)) continue;
                var relativePath = NormalizeEntryPath(entry.FullName);
                if (!relativePath.StartsWith("files/", StringComparison.OrdinalIgnoreCase)) continue;
                var relativeInFiles = relativePath["files/".Length..];
                var ext = Path.GetExtension(entry.Name).ToLowerInvariant();
                if (ext == ".ini") continue;
                if (string.IsNullOrWhiteSpace(relativeInFiles)) continue;
                var normalized = relativeInFiles.Replace('\\', '/');
                if (newRelativeFiles.Add(normalized))
                    newRelativeFilesList.Add(normalized);
            }

            // ID collision — same modId already installed (component-level conflict)
            if (Directory.Exists(targetModDir))
            {
                if (!replace)
                {
                    var existingForId = ModCommands.LoadMetadata(gamePath).FirstOrDefault(m => string.Equals(m.Id, modId, StringComparison.OrdinalIgnoreCase));
                    var conflictIdInfo = new ModConflictInfo
                    {
                        ModId = existingForId?.Id ?? modId,
                        ModName = existingForId?.Name ?? modId,
                        ModVersion = existingForId?.Version ?? "",
                        ConflictingFiles = newRelativeFilesList.ToList(),
                        IsSameMod = true
                    };
                    return new ModInstallResult
                    {
                        Success = false,
                        Conflict = true,
                        IsModConflict = true,
                        Message = $"A mod with this ID is already installed.",
                        ConflictingMods = [conflictIdInfo],
                        NewModName = manifest.Name
                    };
                }

                var existingModsForId = ModCommands.LoadMetadata(gamePath);
                var existingModForId = existingModsForId.FirstOrDefault(m => string.Equals(m.Id, modId, StringComparison.OrdinalIgnoreCase));
                if (existingModForId != null)
                {
                    await RemoveAsync(gamePath, existingModForId);
                    existingModsForId.Remove(existingModForId);
                    ModCommands.SaveMetadata(gamePath, existingModsForId);
                }
                else
                {
                    try { Directory.Delete(targetModDir, recursive: true); } catch { }
                }
            }

            // File-overlap detection: compare new mod's files against every installed mod's OwnedFiles
            // (files they currently manage, not just historically installed)
            var metadataMods = ModCommands.LoadMetadata(gamePath);
            var conflicts = new List<ModConflictInfo>();
            foreach (var existingMod in metadataMods)
            {
                if (string.Equals(existingMod.Id, modId, StringComparison.OrdinalIgnoreCase)) continue;

                var overlapping = new List<string>();
                foreach (var ownedFile in existingMod.OwnedFiles)
                {
                    var ext = Path.GetExtension(ownedFile).ToLowerInvariant();
                    if (ext == ".ini") continue;
                    string relativeExisting;
                    try { relativeExisting = Path.GetRelativePath(resolvedGame, ownedFile).Replace('\\', '/'); }
                    catch { continue; }
                    if (newRelativeFiles.Contains(relativeExisting))
                        overlapping.Add(relativeExisting);
                }
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
                        Message = $"Mod '{manifest.Name}' conflicts with {names} (overlapping files: {files}).",
                        ConflictingMods = conflicts,
                        NewModName = manifest.Name
                    };
                }

                if (replace)
                {
                    // replace requested — remove all conflicting mods before installing new one
                    var allMods = ModCommands.LoadMetadata(gamePath);
                    foreach (var c in conflicts)
                    {
                        var modToRemove = allMods.FirstOrDefault(m => string.Equals(m.Id, c.ModId, StringComparison.OrdinalIgnoreCase));
                        if (modToRemove != null)
                        {
                            await RemoveAsync(gamePath, modToRemove);
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

            Directory.CreateDirectory(targetModDir);

            var installedFiles = new List<string>();

            foreach (var entry in archive.Entries)
            {
                if (IsDirectoryEntry(entry)) continue;

                var relativePath = NormalizeEntryPath(entry.FullName);
                if (relativePath.StartsWith("files/"))
                {
                    var relativeInFiles = relativePath["files/".Length..];
                    var ext = Path.GetExtension(entry.Name).ToLowerInvariant();

                    if (ext != ".ini")
                    {
                        var destGamePath = ResolveUnder(resolvedGame, relativeInFiles);
                        var destDir = Path.GetDirectoryName(destGamePath);
                        if (destDir != null) Directory.CreateDirectory(destDir);

                        if (File.Exists(destGamePath))
                        {
                            var backupPath = TempestPathUtility.GetLocalV2BackupPath(resolvedGame, relativeInFiles);
                            var backupDir = Path.GetDirectoryName(backupPath);
                            if (backupDir != null) Directory.CreateDirectory(backupDir);

                            if (File.Exists(backupPath))
                            {
                                // Backup already exists (the pristine original), do not overwrite it.
                                // Just delete the existing destination file to copy/write the new mod file.
                                File.Delete(destGamePath);
                            }
                            else
                            {
                                // No backup exists yet - this is the first mod touching this file.
                                // Copy the current game file (pristine or previously restored) as backup.
                                File.Copy(destGamePath, backupPath);
                            }
                        }

                        // Write mod file to game directory
                        await using (var srcStream = await entry.OpenAsync())
                        await using (var destStream = File.Create(destGamePath))
                        {
                            await srcStream.CopyToAsync(destStream);
                        }

                        // Keep a snapshot in mod dir for re-enable
                        var modSnapshotPath = Path.Combine(targetModDir, "files", relativeInFiles);
                        var modSnapshotDir = Path.GetDirectoryName(modSnapshotPath);
                        if (modSnapshotDir != null) Directory.CreateDirectory(modSnapshotDir);
                        File.Copy(destGamePath, modSnapshotPath, overwrite: true);

                        installedFiles.Add(destGamePath);
                    }
                    else
                    {
                        var destGamePath = ResolveUnder(resolvedGame, relativeInFiles);
                        var destDir = Path.GetDirectoryName(destGamePath);
                        if (destDir != null) Directory.CreateDirectory(destDir);

                        var modFilesIniPath = Path.Combine(targetModDir, "files", relativeInFiles);
                        var modFilesIniDir = Path.GetDirectoryName(modFilesIniPath);
                        if (modFilesIniDir != null) Directory.CreateDirectory(modFilesIniDir);

                        await using (var srcStream = await entry.OpenAsync())
                        await using (var destStream = File.Create(modFilesIniPath))
                        {
                            await srcStream.CopyToAsync(destStream);
                        }

                        if (File.Exists(destGamePath))
                        {
                            var gameIniLines = IniPatcher.Parse(destGamePath);
                            var modIniLines = IniPatcher.Parse(modFilesIniPath);
                            var backupLines = new List<IniLine>();

                            ApplyModIniToGameIni(gameIniLines, modIniLines, backupLines);

                            var iniBackupPath = TempestPathUtility.GetLocalV2IniBackupPath(resolvedGame, modId, relativeInFiles);
                            var iniBackupDir = Path.GetDirectoryName(iniBackupPath);
                            if (iniBackupDir != null) Directory.CreateDirectory(iniBackupDir);

                            if (backupLines.Count > 0)
                            {
                                IniPatcher.Save(iniBackupPath, backupLines);
                            }

                            IniPatcher.Save(destGamePath, gameIniLines);
                        }
                        else
                        {
                            File.Copy(modFilesIniPath, destGamePath);
                        }

                        installedFiles.Add(destGamePath);
                    }
                }
                else
                {
                    var destModPath = ResolveUnder(targetModDir, relativePath);
                    var destDir = Path.GetDirectoryName(destModPath);
                    if (destDir != null) Directory.CreateDirectory(destDir);

                    await using var srcStream = await entry.OpenAsync();
                    await using var destStream = File.Create(destModPath);

                    await srcStream.CopyToAsync(destStream);
                }
            }

var authorString = string.Join(", ", manifest.Authors.Select(a => a.Name));
            if (string.IsNullOrEmpty(authorString)) authorString = "Unknown";

            var ownedFiles = installedFiles
                .Where(f => Path.GetExtension(f).ToLowerInvariant() != ".ini")
                .ToList();

            var modRecord = new ModRecord
            {
                Id = modId,
                Name = manifest.Name,
                Author = authorString,
                Authors = manifest.Authors,
                Version = string.IsNullOrEmpty(manifest.Version) ? "1.0.0" : manifest.Version,
                Enabled = true,
                Kind = "V2",
                OriginalPath = modFilePath,
                InstalledFiles = installedFiles,
                OwnedFiles = ownedFiles,
                MetadataVersion = 2,
                Readme = manifest.Readme,
                Signer = verification.Key == null ? "" : ModSigning.Fingerprint(verification.Key),
                Verified = true,
                ManifestHash = checksumsEntry == null ? "" : Convert.ToHexStringLower(SHA256.HashData(Encoding.UTF8.GetBytes(await ReadEntryAsync(checksumsEntry))))
            };

            return new ModInstallResult
            {
                Success = true,
                Message = "Mod installed successfully",
                Mod = modRecord
            };
        }
        catch (Exception ex)
        {
            return new ModInstallResult
            {
                Success = false,
                Message = $"Failed to install V2 mod: {ex.Message}"
            };
        }
    }

    public Task RemoveAsync(string gamePath, ModRecord mod)
    {
        var resolvedGame = GameFolderResolver.Resolve(gamePath);
        var modId = mod.Id;

        var targetModDir = TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, modId);
        var targetFilesDir = Path.Combine(targetModDir, "files");

        if (Directory.Exists(targetFilesDir))
        {
            var filesInFilesDir = Directory.GetFiles(targetFilesDir, "*.ini", SearchOption.AllDirectories);
            foreach (var modIniFile in filesInFilesDir)
            {
                var relativePathFromFiles = Path.GetRelativePath(targetFilesDir, modIniFile);
                var destGamePath = Path.Combine(resolvedGame, relativePathFromFiles);

                if (!File.Exists(destGamePath)) continue;

                var gameIniLines = IniPatcher.Parse(destGamePath);
                var modIniLines = IniPatcher.Parse(modIniFile);

                var iniBackupPath = TempestPathUtility.GetLocalV2IniBackupPath(resolvedGame, modId, relativePathFromFiles);
                var backupLines = File.Exists(iniBackupPath) ? IniPatcher.Parse(iniBackupPath) : [];

                RestoreGameIni(gameIniLines, modIniLines, backupLines);

                IniPatcher.Save(destGamePath, gameIniLines);
            }
        }

        // Only restore files this mod currently owns
        foreach (var file in mod.OwnedFiles)
        {
            var relativePathFromGame = Path.GetRelativePath(resolvedGame, file);
            var backupPath = TempestPathUtility.GetLocalV2BackupPath(resolvedGame, relativePathFromGame);

            if (File.Exists(backupPath))
            {
                try
                {
                    var destDir = Path.GetDirectoryName(file);
                    if (destDir != null) Directory.CreateDirectory(destDir);

                    if (File.Exists(file)) File.Delete(file);
                    File.Move(backupPath, file, overwrite: true);
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine($"Warning: Failed to restore backup {backupPath} to {file}: {ex.Message}");
                }
            }
            else
            {
                if (!File.Exists(file)) continue;

                try { File.Delete(file); }
                catch
                {
                    // ignored
                }
            }
        }

        var modV2BackupDir = TempestPathUtility.GetLocalV2BackupDirectory(resolvedGame, modId);
        var modV2IniBackupDir = TempestPathUtility.GetLocalV2IniBackupDirectory(resolvedGame, modId);

        try
        {
            if (Directory.Exists(targetModDir)) Directory.Delete(targetModDir, recursive: true);
            if (Directory.Exists(modV2BackupDir)) Directory.Delete(modV2BackupDir, recursive: true);
            if (Directory.Exists(modV2IniBackupDir)) Directory.Delete(modV2IniBackupDir, recursive: true);
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Warning: Failed to delete directories during uninstall: {ex.Message}");
        }

        return Task.CompletedTask;
    }

    public Task DisableAsync(string gamePath, ModRecord mod)
    {
        var resolvedGame = GameFolderResolver.Resolve(gamePath);
        var modId = mod.Id;

        var targetModDir = TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, modId);
        var targetFilesDir = Path.Combine(targetModDir, "files");

        // Restore INI patches (keep the .ini files in mod dir, keep ini-backups intact)
        if (Directory.Exists(targetFilesDir))
        {
            var filesInFilesDir = Directory.GetFiles(targetFilesDir, "*.ini", SearchOption.AllDirectories);
            foreach (var modIniFile in filesInFilesDir)
            {
                var relativePathFromFiles = Path.GetRelativePath(targetFilesDir, modIniFile);
                var destGamePath = Path.Combine(resolvedGame, relativePathFromFiles);

                if (!File.Exists(destGamePath)) continue;

                var gameIniLines = IniPatcher.Parse(destGamePath);
                var modIniLines = IniPatcher.Parse(modIniFile);

                var iniBackupPath = TempestPathUtility.GetLocalV2IniBackupPath(resolvedGame, modId, relativePathFromFiles);
                var backupLines = File.Exists(iniBackupPath) ? IniPatcher.Parse(iniBackupPath) : [];

                RestoreGameIni(gameIniLines, modIniLines, backupLines);

                IniPatcher.Save(destGamePath, gameIniLines);
            }
        }

        // Restore non-ini files this mod owns: COPY from backup (keep backup intact for re-enable)
        foreach (var file in mod.OwnedFiles)
        {
            var relativePathFromGame = Path.GetRelativePath(resolvedGame, file);
            var backupPath = TempestPathUtility.GetLocalV2BackupPath(resolvedGame, relativePathFromGame);

            if (!File.Exists(backupPath)) continue;

            try
            {
                var destDir = Path.GetDirectoryName(file);
                if (destDir != null) Directory.CreateDirectory(destDir);

                if (File.Exists(file)) File.Delete(file);
                File.Copy(backupPath, file);
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine($"Warning: Failed to restore backup {backupPath} to {file}: {ex.Message}");
            }
        }

        // Keep mod dir, backup dirs, and ini-backup dirs intact for re-enable

        return Task.CompletedTask;
    }

    public Task EnableAsync(string gamePath, ModRecord mod)
    {
        var resolvedGame = GameFolderResolver.Resolve(gamePath);
        var modId = mod.Id;

        var targetModDir = TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, modId);
        var snapshotFilesDir = Path.Combine(targetModDir, "files");

        if (!Directory.Exists(targetModDir))
        {
            Console.Error.WriteLine($"Warning: Mod directory not found for '{modId}'. Cannot enable.");
            return Task.CompletedTask;
        }

        // Re-apply non-INI files this mod owns from the mod snapshot directory
        foreach (var file in mod.OwnedFiles)
        {
            var relativePathFromGame = Path.GetRelativePath(resolvedGame, file);
            var snapshotPath = Path.Combine(snapshotFilesDir, relativePathFromGame);

            if (!File.Exists(snapshotPath))
            {
                Console.Error.WriteLine($"Warning: Snapshot file not found for '{relativePathFromGame}'. Mod may have been installed before snapshot support was added.");
                continue;
            }

            try
            {
                var destDir = Path.GetDirectoryName(file);
                if (destDir != null) Directory.CreateDirectory(destDir);

                if (File.Exists(file)) File.Delete(file);
                File.Copy(snapshotPath, file);
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine($"Warning: Failed to enable file {file}: {ex.Message}");
            }
        }

        // Re-apply INI patches
        if (Directory.Exists(snapshotFilesDir))
        {
            var filesInFilesDir = Directory.GetFiles(snapshotFilesDir, "*.ini", SearchOption.AllDirectories);
            foreach (var modIniFile in filesInFilesDir)
            {
                var relativePathFromFiles = Path.GetRelativePath(snapshotFilesDir, modIniFile);
                var destGamePath = Path.Combine(resolvedGame, relativePathFromFiles);

                if (File.Exists(destGamePath))
                {
                    var gameIniLines = IniPatcher.Parse(destGamePath);
                    var modIniLines = IniPatcher.Parse(modIniFile);
                    var backupLines = new List<IniLine>();

                    ApplyModIniToGameIni(gameIniLines, modIniLines, backupLines);

                    var iniBackupPath = TempestPathUtility.GetLocalV2IniBackupPath(resolvedGame, modId, relativePathFromFiles);
                    var iniBackupDir = Path.GetDirectoryName(iniBackupPath);
                    if (iniBackupDir != null) Directory.CreateDirectory(iniBackupDir);

                    if (backupLines.Count > 0)
                    {
                        IniPatcher.Save(iniBackupPath, backupLines);
                    }

                    IniPatcher.Save(destGamePath, gameIniLines);
                }
                else
                {
                    // Game INI didn't exist before — just copy the mod's snapshot
                    var destDir = Path.GetDirectoryName(destGamePath);
                    if (destDir != null) Directory.CreateDirectory(destDir);
                    File.Copy(modIniFile, destGamePath);
                }
            }
        }

        return Task.CompletedTask;
    }

    private static void ApplyModIniToGameIni(List<IniLine> gameIniLines, List<IniLine> modIniLines, List<IniLine> backupLines)
    {
        string? currentModSection = null;
        foreach (var line in modIniLines)
        {
            switch (line.Type)
            {
                case IniLineType.Section:
                    currentModSection = line.SectionName;
                    break;
                case IniLineType.Entry when currentModSection != null && line.Key != null:
                    if (line.Prefix is "+" or "-")
                    {
                        IniPatcher.ApplyPatch(gameIniLines, currentModSection, line.Prefix, line.Key, line.Value ?? "");
                    }
                    else
                    {
                        var gameIndex = IniPatcher.FindKeyIndex(gameIniLines, currentModSection, line.Key);
                        if (gameIndex >= 0)
                        {
                            var existingEntry = gameIniLines[gameIndex];
                            IniPatcher.ApplyPatch(backupLines, currentModSection, existingEntry.Prefix ?? "", existingEntry.Key ?? "", existingEntry.Value ?? "");
                            gameIniLines[gameIndex] = IniPatcher.CreateEntry(currentModSection, line.Key, line.Value ?? "", line.Prefix);
                        }
                        else
                        {
                            IniPatcher.ApplyPatch(gameIniLines, currentModSection, line.Prefix ?? "", line.Key, line.Value ?? "");
                        }
                    }
                    break;
            }
        }
    }

    private static void RestoreGameIni(List<IniLine> gameIniLines, List<IniLine> modIniLines, List<IniLine> backupLines)
    {
        string? currentModSection = null;
        foreach (var line in modIniLines)
        {
            switch (line.Type)
            {
                case IniLineType.Section:
                    currentModSection = line.SectionName;
                    break;
                case IniLineType.Entry when currentModSection != null && line.Key != null:
                    var backupIndex = IniPatcher.FindKeyIndex(backupLines, currentModSection, line.Key);
                    if (backupIndex >= 0)
                    {
                        var gameIndex = IniPatcher.FindKeyIndex(gameIniLines, currentModSection, line.Key);
                        if (gameIndex >= 0)
                            gameIniLines[gameIndex] = backupLines[backupIndex];
                    }
                    else if (line.Prefix == "+")
                    {
                        var value = line.Value ?? "";
                        IniPatcher.RemoveAllInSection(gameIniLines, currentModSection, l =>
                            l.Type == IniLineType.Entry &&
                            string.Equals(l.Key, line.Key, StringComparison.OrdinalIgnoreCase) &&
                            (string.Equals(l.Value, value, StringComparison.OrdinalIgnoreCase) ||
                             string.Equals(l.Value, value + " ; tempest", StringComparison.OrdinalIgnoreCase)));
                    }
                    else if (line.Prefix != "-")
                    {
                        IniPatcher.RemoveAllInSection(gameIniLines, currentModSection, l =>
                            l.Type == IniLineType.Entry &&
                            string.Equals(l.Key, line.Key, StringComparison.OrdinalIgnoreCase));
                    }
                    break;
            }
        }
    }

    private static Dictionary<string, string> ParseChecksumMap(string checksumsContent)
    {
        return checksumsContent
            .Split(["\r\n", "\r", "\n"], StringSplitOptions.RemoveEmptyEntries)
            .Select(line => line.Split(' ', StringSplitOptions.RemoveEmptyEntries))
            .Where(parts => parts.Length >= 2)
            .ToDictionary(
                parts => string.Join(" ", parts.Skip(1)).TrimStart('*').Trim().Replace('\\', '/'),
                parts => parts[0].Trim().ToLowerInvariant(),
                StringComparer.OrdinalIgnoreCase);
    }

    private static async Task VerifyArchiveChecksumsAsync(ZipArchive archive, Dictionary<string, string> checksumMap)
    {
        foreach (var entry in archive.Entries)
        {
            if (IsDirectoryEntry(entry)) continue;

            var relativePath = NormalizeEntryPath(entry.FullName);
            if (ModSigning.IsSealEntry(relativePath)) continue;

            if (!checksumMap.TryGetValue(relativePath, out var expectedHash))
                throw new Exception($"File '{relativePath}' is not present in CHECKSUMS.");

            await using var stream = await entry.OpenAsync();
            using var sha256 = SHA256.Create();
            var actualHash = Convert.ToHexString(await sha256.ComputeHashAsync(stream)).ToLowerInvariant();

            if (actualHash != expectedHash)
                throw new Exception($"File '{relativePath}' checksum mismatch! Expected: {expectedHash}, Actual: {actualHash}");
        }
    }

    public static ModManifest ParseManifest(string tomlText)
    {
        var manifest = new ModManifest();
        try
        {
            var model = TomlSerializer.Deserialize<TomlTable>(tomlText, ModTomlContext.Default.TomlTable);
            if (model != null && model.TryGetValue("mod", out var modObj) && modObj is TomlTable modTable)
            {
                if (modTable.TryGetValue("id", out var idVal)) manifest.Id = idVal?.ToString() ?? "";
			if (modTable.TryGetValue("name", out var nameVal)) manifest.Name = nameVal?.ToString() ?? "";
				if (modTable.TryGetValue("version", out var versionVal)) manifest.Version = versionVal?.ToString() ?? "1.0.0";
				if (modTable.TryGetValue("icon", out var iconVal)) manifest.Icon = iconVal?.ToString() ?? "";
                if (modTable.TryGetValue("readme", out var readmeVal)) manifest.Readme = readmeVal?.ToString() ?? "";

                if (modTable.TryGetValue("authors", out var authorsVal))
                {
                    if (authorsVal is TomlArray array)
                    {
                        foreach (var item in array)
                        {
                            if (item != null)
                            {
                                manifest.Authors.Add(new ModAuthor { Name = item.ToString() ?? "" });
                            }
                        }
                    }
                    else if (authorsVal is TomlTableArray tableArray)
                    {
                        foreach (var item in tableArray)
                        {
                            var author = new ModAuthor();
                            if (item.TryGetValue("name", out var name)) author.Name = name?.ToString() ?? "";
                            if (item.TryGetValue("link", out var link)) author.Link = link?.ToString() ?? "";
                            if (item.TryGetValue("avatar", out var avatar)) author.Avatar = avatar?.ToString() ?? "";
                            manifest.Authors.Add(author);
                        }
                    }
                }
            }
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Error parsing manifest.toml with Tomlyn: {ex.Message}");
        }

        return manifest;
    }
}

public class ModManifest
{
	public string Id { get; set; } = string.Empty;
	public string Name { get; set; } = string.Empty;
	public string Version { get; set; } = "1.0.0";
	public string Icon { get; set; } = string.Empty;
	public string Readme { get; set; } = string.Empty;
	public List<ModAuthor> Authors { get; set; } = [];
}

public class ModAuthor
{
    public string Name { get; set; } = string.Empty;
    public string Link { get; set; } = string.Empty;
    public string Avatar { get; set; } = string.Empty;
}
