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
            else if (relativePath.StartsWith("deltas/", StringComparison.OrdinalIgnoreCase))
            {
                // `deltas/<game path>.tdelta` is applied to that game file.
                var target = DeltaResolver.TargetOf(relativePath) ?? throw new Exception($"'{relativePath}' is not a delta (expected deltas/<game path>.tdelta).");
                var dest = ResolveUnder(resolvedGame, target);
                if (IsSameOrBelow(dest, tempestDir, StringComparison.OrdinalIgnoreCase))
                    throw new Exception($"'{relativePath}' would change a file in the game's .tempest folder.");
                ResolveUnder(targetModDir, relativePath);
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

    /// <summary>
    /// Files an install would overwrite or delete (the package's destinations, the mod's folder,
    /// and the files and folders of <paramref name="removedMods"/>) that another process holds
    /// open without sharing write access, relative to the game folder. Windows only: elsewhere an
    /// open file does not stop it being replaced.
    /// </summary>
    private static List<string> FindFilesInUse(ZipArchive archive, string resolvedGame, string targetModDir, IEnumerable<ModRecord> removedMods)
    {
        if (!OperatingSystem.IsWindows()) return [];

        var candidates = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        void AddFolder(string dir)
        {
            if (!Directory.Exists(dir)) return;
            foreach (var file in Directory.EnumerateFiles(dir, "*", SearchOption.AllDirectories)) candidates.Add(file);
        }

        foreach (var entry in archive.Entries)
        {
            if (IsDirectoryEntry(entry)) continue;
            var relativePath = NormalizeEntryPath(entry.FullName);
            candidates.Add(relativePath.StartsWith("files/", StringComparison.OrdinalIgnoreCase)
                ? ResolveUnder(resolvedGame, relativePath["files/".Length..])
                : ResolveUnder(targetModDir, relativePath));
            // The game file a delta rebuilds is rewritten too.
            if (DeltaResolver.TargetOf(relativePath) is { } target) candidates.Add(ResolveUnder(resolvedGame, target));
        }
        AddFolder(targetModDir);
        foreach (var mod in removedMods)
        {
            foreach (var file in mod.OwnedFiles) candidates.Add(Path.GetFullPath(file, resolvedGame));
            AddFolder(TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, mod.Id));
        }

        return candidates
            .Where(file => File.Exists(file) && IsInUse(file))
            .Select(file => Path.GetRelativePath(resolvedGame, file).Replace('\\', '/'))
            .Order(StringComparer.OrdinalIgnoreCase)
            .ToList();
    }

    /// <summary>Another process has the file open without letting others write it (sharing or lock violation, or a mapped image such as a loaded DLL).</summary>
    private static bool IsInUse(string file)
    {
        try
        {
            using var _ = new FileStream(file, FileMode.Open, FileAccess.ReadWrite, FileShare.ReadWrite | FileShare.Delete);
            return false;
        }
        catch (IOException ex) when ((ex.HResult & 0xFFFF) is 32 or 33 or 1224)
        {
            return true;
        }
        catch
        {
            // Read-only or inaccessible: not something waiting for the game fixes.
            return false;
        }
    }

    private static async Task<string> ReadEntryAsync(ZipArchiveEntry entry)
    {
        using var reader = new StreamReader(await entry.OpenAsync(), Encoding.UTF8);
        return await reader.ReadToEndAsync();
    }

    public Task<ModInstallResult> InstallAsync(string gamePath, string modFilePath, bool replace, bool stack, bool allowUntrusted, ModLock modLock) =>
        InstallAsync(gamePath, modFilePath, replace, stack, allowUntrusted, modLock, ignoreTempestVersion: false);

    /// <param name="defer">Stop after every check passed, before anything is written, with a <see cref="ModInstallResult.Deferred"/> result.</param>
    public async Task<ModInstallResult> InstallAsync(string gamePath, string modFilePath, bool replace, bool stack, bool allowUntrusted, ModLock modLock, bool ignoreTempestVersion, bool defer = false)
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
            if (manifest.InvalidDependencies.Count > 0)
            {
                throw new Exception($"manifest.toml: invalid dependencies {string.Join(", ", manifest.InvalidDependencies.Select(d => $"\"{d}\""))} (expected entries like \"tempest-scripting>=1.0.0\")");
            }
            // A mod written for a newer Tempest: the launcher tells the user to update rather
            // than install something that will not run. Unstamped developer builds skip this.
            var tempestRequired = ModDependencies.UnmetTempestRequirement(manifest.Dependencies, ModDependencies.TempestVersion);
            if (tempestRequired != null && !ignoreTempestVersion)
            {
                return new ModInstallResult
                {
                    Success = false,
                    TempestRequired = tempestRequired,
                    Message = $"'{manifest.Name}' needs Tempest {tempestRequired}; this is Tempest {ModDependencies.TempestVersion}. Update Tempest, or pass --ignore-tempest-version.",
                };
            }

            // Mods without code (no DLL, no Lua) cannot do harm and install whoever signed them.
            // With code: a trusted signature installs, an untrusted one needs allowUntrusted (the
            // launcher asks first), anything else is refused.
            var code = await FindCodeAsync(archive);
            var trust = code.Count > 0 ? await TrustStore.LoadAsync(resolvedGame) : null;

            var checksumsEntry = archive.GetEntry(ModSigning.ChecksumsName);
            var checksumsContent = checksumsEntry != null ? await ReadEntryAsync(checksumsEntry) : null;
            var verification = new ModSigning.Verification(ModSigning.Trust.Unsigned, null, null);
            if (checksumsContent != null)
            {
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

            // Deltas conflict per object or row, not per file: two mods changing different
            // objects of the same package both install. Keys by game-relative target.
            var newDeltas = new Dictionary<string, HashSet<string>>(StringComparer.OrdinalIgnoreCase);
            foreach (var entry in archive.Entries)
            {
                if (IsDirectoryEntry(entry)) continue;
                var target = DeltaResolver.TargetOf(NormalizeEntryPath(entry.FullName));
                if (target == null) continue;
                List<string> keys;
                try
                {
                    await using var stream = await entry.OpenAsync();
                    using var buffer = new MemoryStream();
                    await stream.CopyToAsync(buffer);
                    buffer.Position = 0;
                    keys = DeltaResolver.Keys(buffer);
                }
                catch (Exception ex)
                {
                    throw new Exception($"'{entry.FullName}' is not a readable delta: {ex.Message}");
                }
                if (!newDeltas.TryGetValue(target, out var set)) newDeltas[target] = set = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
                set.UnionWith(keys);
            }

            // ID collision — same modId already installed (component-level conflict)
            ModInstallResult IdConflict()
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
            if (!replace && Directory.Exists(targetModDir)) return IdConflict();

            // Hashing every file is the slow part for big packages, so it waits until the
            // signature gate and the "already installed" prompt passed (the launcher re-runs
            // install after either), and still runs before anything on disk is touched.
            if (checksumsContent != null)
                await VerifyArchiveChecksumsAsync(archive, ParseChecksumMap(checksumsContent));

            // Everything above runs alongside other installs, which may have installed this id
            // meanwhile; from here on installed mods are read and changed under the lock.
            await modLock.EnterAsync();
            var idInstalled = Directory.Exists(targetModDir);
            if (idInstalled && !replace) return IdConflict();

            // File-overlap detection: compare new mod's files against every installed mod's OwnedFiles
            // (files they currently manage, not just historically installed). Only reads: the mod
            // being replaced (same id) is skipped, so it may still be installed at this point.
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
                // Both mods change the same object or row of a delta-managed file.
                foreach (var deltaFile in existingMod.DeltaFiles)
                {
                    string relativeExisting;
                    try { relativeExisting = Path.GetRelativePath(resolvedGame, deltaFile).Replace('\\', '/'); }
                    catch { continue; }
                    if (!newDeltas.TryGetValue(relativeExisting, out var newKeys)) continue;
                    var existingDelta = Path.Combine(TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, existingMod.Id), "deltas", relativeExisting + ".tdelta");
                    if (!File.Exists(existingDelta)) continue;
                    List<string> existingKeys;
                    try
                    {
                        await using var stream = File.OpenRead(existingDelta);
                        existingKeys = DeltaResolver.Keys(stream);
                    }
                    catch { continue; }
                    foreach (var key in existingKeys.Where(newKeys.Contains).Order(StringComparer.OrdinalIgnoreCase))
                        overlapping.Add($"{relativeExisting}: {key}");
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

            if (conflicts.Count > 0 && !replace && !stack)
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

            // Nothing on disk has changed yet. A file the install would overwrite or delete that
            // another process holds open (a running game keeps its DLLs and packages open) would
            // make it fail halfway, with the replaced mod already gone; refuse it whole instead.
            var removedMods = metadataMods.Where(m =>
                string.Equals(m.Id, modId, StringComparison.OrdinalIgnoreCase) ||
                replace && conflicts.Any(c => string.Equals(c.ModId, m.Id, StringComparison.OrdinalIgnoreCase)));
            var inUse = FindFilesInUse(archive, resolvedGame, targetModDir, removedMods);
            if (inUse.Count > 0)
            {
                var shownFiles = string.Join(", ", inUse.Take(3)) + (inUse.Count > 3 ? $" and {inUse.Count - 3} more" : "");
                return new ModInstallResult
                {
                    Success = false,
                    FilesInUse = true,
                    LockedFiles = inUse,
                    Message = $"'{manifest.Name}' cannot be installed while {shownFiles} {(inUse.Count == 1 ? "is" : "are")} in use, most likely by the running game. Close it and install again.",
                };
            }
            if (defer)
            {
                return new ModInstallResult
                {
                    Success = false,
                    Deferred = true,
                    Message = $"'{manifest.Name}' can be installed; nothing was written (--defer).",
                };
            }

            // An update or reinstall stays where the mod was first added.
            DateTime? installedAt = null;
            if (idInstalled)
            {
                var existingModsForId = ModCommands.LoadMetadata(gamePath);
                var existingModForId = existingModsForId.FirstOrDefault(m => string.Equals(m.Id, modId, StringComparison.OrdinalIgnoreCase));
                if (existingModForId != null)
                {
                    installedAt = existingModForId.InstalledAt;
                    await RemoveAsync(gamePath, existingModForId);
                    existingModsForId.Remove(existingModForId);
                    ModCommands.SaveMetadata(gamePath, existingModsForId);
                }
                else
                {
                    try { Directory.Delete(targetModDir, recursive: true); } catch { }
                }
            }

            if (conflicts.Count > 0)
            {
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
            // The game files the deltas apply to; built by the resolver once this record is saved.
            var deltaFiles = newDeltas.Keys.Select(t => ResolveUnder(resolvedGame, t)).Order(StringComparer.OrdinalIgnoreCase).ToList();

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
                DeltaFiles = deltaFiles,
                MetadataVersion = 2,
                Readme = manifest.Readme,
                Signer = verification.Key == null ? "" : ModSigning.Fingerprint(verification.Key),
                Verified = true,
                ManifestHash = checksumsEntry == null ? "" : Convert.ToHexStringLower(SHA256.HashData(Encoding.UTF8.GetBytes(await ReadEntryAsync(checksumsEntry)))),
                Dependencies = manifest.Dependencies,
                InstalledAt = installedAt ?? DateTime.UtcNow,
            };

            // Installed either way; what it still needs is reported so the launcher (or
            // `mod install <id>`, which resolves them from the repository first) can act on it.
            var missing = ModDependencies.Missing(manifest.Dependencies, ModCommands.LoadMetadata(gamePath));
            return new ModInstallResult
            {
                Success = true,
                Message = missing.Count == 0
                    ? "Mod installed successfully"
                    : $"Mod installed successfully; it needs {string.Join(", ", missing)}, which is not installed and enabled.",
                Mod = modRecord,
                MissingDependencies = missing.Count == 0 ? null : missing,
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

                // Under [mod] as documented; a bare top-level array is taken too.
                if (modTable.TryGetValue("dependencies", out var depsVal) || model.TryGetValue("dependencies", out depsVal))
                {
                    if (depsVal is TomlArray depsArray)
                    {
                        foreach (var item in depsArray)
                        {
                            var text = item?.ToString() ?? "";
                            var dep = SemverRange.ParseDependency(text);
                            if (dep == null || !SemverRange.IsValid(dep.Requirement) || manifest.Dependencies.Any(d => d.Id == dep.Id))
                                manifest.InvalidDependencies.Add(text);
                            else
                                manifest.Dependencies.Add(dep);
                        }
                    }
                    else manifest.InvalidDependencies.Add(depsVal?.ToString() ?? "");
                }

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
	/// <summary>`dependencies = ["tempest>=1.1.0", "tempest-scripting>=1.0.0"]`, parsed; see <see cref="SemverRange"/>.</summary>
	public List<ModDependency> Dependencies { get; set; } = [];
	/// <summary>Entries of `dependencies` that did not parse, as written.</summary>
	public List<string> InvalidDependencies { get; set; } = [];
}

public class ModAuthor
{
    public string Name { get; set; } = string.Empty;
    public string Link { get; set; } = string.Empty;
    public string Avatar { get; set; } = string.Empty;
}
