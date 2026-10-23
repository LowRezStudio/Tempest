using System.Security.Cryptography;
using System.Text;

namespace Tempest.CLI.Mods;

/// <summary>
/// Launch-time integrity for installed V2 mods. Signing is checked when a package is
/// installed; this re-checks it right before the game reads any mod file, so files
/// edited after install (scripts, DLLs, snapshots, installed game files) are caught.
/// The anchors are the package's own <c>CHECKSUMS</c> / <c>CHECKSUMS.asc</c> /
/// <c>SIGNER.pub</c>, which were written verbatim into the mod folder at install:
/// the signature proves CHECKSUMS has not been regenerated, and CHECKSUMS covers
/// every other file of the package, including SIGNER.pub itself.
/// </summary>
internal static class ModIntegrity
{
    private static readonly Dictionary<string, string> EmptyChecksums = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// Verifies every enabled V2 mod and disables the ones that fail (game files are
    /// restored from their backups first). Survivors get <see cref="ModRecord.Verified"/>
    /// set, which is what the runtime (Tempest.Scripting) and DLL injection trust.
    /// </summary>
    public static async Task VerifyAtLaunchAsync(string gamePath)
    {
        try
        {
            if (!File.Exists(ModCommands.GetMetadataPath(gamePath))) return;
            using var modLock = await ModLock.AcquireAsync(gamePath);
            var mods = ModCommands.LoadMetadata(gamePath);
            var resolvedGame = GameFolderResolver.Resolve(gamePath);
            var v2Enabled = mods
                .Where(m => m.Enabled && string.Equals(m.Kind, "V2", StringComparison.OrdinalIgnoreCase))
                .ToList();
            if (v2Enabled.Count == 0) return;

            TrustStore? trust = null;
            var changed = false;

            // Only mods no other enabled V2 mod writes the same game file with are checked
            // in the game folder; overlapping installs overwrite each other by design.
            var gameFileOwners = new Dictionary<string, int>(StringComparer.OrdinalIgnoreCase);
            foreach (var mod in v2Enabled)
            {
                foreach (var file in mod.OwnedFiles)
                {
                    var relative = TryGameRelative(resolvedGame, file);
                    if (relative != null) gameFileOwners[relative] = gameFileOwners.TryGetValue(relative, out var n) ? n + 1 : 1;
                }
            }

            foreach (var mod in v2Enabled)
            {
                // Only a signed mod is checked against the key lists, and loading them is a network round-trip.
                if (mod.Signer.Length > 0) trust ??= await TrustStore.LoadAsync(resolvedGame);
                var problem = await VerifyInstalledAsync(resolvedGame, mod, trust, gameFileOwners);
                if (problem == null)
                {
                    if (!mod.Verified || mod.ManifestHash.Length == 0) changed = true;
                    mod.Verified = true;
                    continue;
                }

                await ModCommands.CreateInstaller(mod).DisableAsync(gamePath, mod);
                mod.Enabled = false;
                changed = true;
                await Console.Error.WriteLineAsync(
                    $"Warning: disabled '{mod.Name}': its files no longer match what was signed when it was installed ({problem}). " +
                    "Modified mods are not run; install the mod again to run a different build.");
            }

            if (changed) ModCommands.SaveMetadata(gamePath, mods);
        }
        catch (Exception ex)
        {
            await Console.Error.WriteLineAsync($"Warning: could not verify installed mods before launch: {ex.Message}");
        }
    }

    /// <summary>Null when the installed files still match the signed CHECKSUMS, else why they do not.</summary>
    internal static async Task<string?> VerifyInstalledAsync(
        string resolvedGame, ModRecord mod, TrustStore? trust, IReadOnlyDictionary<string, int> gameFileOwners)
    {
        var modDir = TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, mod.Id);
        if (!Directory.Exists(modDir))
        {
            // Nothing on disk to run; nothing to verify either.
            return null;
        }

        var checksumsPath = Path.Combine(modDir, ModSigning.ChecksumsName);
        if (!File.Exists(checksumsPath))
        {
            // Unsealed install (no CHECKSUMS shipped): allowed only for a mod that never held code.
            if (mod.Signer.Length > 0) return $"{ModSigning.ChecksumsName} is missing from the mod folder; reinstall the mod";
            mod.ManifestHash = "";
            return AddedCodeProblemAsync(modDir, EmptyChecksums, scopedToModFiles: false);
        }

        var checksums = await File.ReadAllTextAsync(checksumsPath);
        mod.ManifestHash = Convert.ToHexStringLower(SHA256.HashData(Encoding.UTF8.GetBytes(checksums)));

        // Signature over CHECKSUMS, checked the same way as at install time.
        if (mod.Signer.Length > 0)
        {
            var ascPath = Path.Combine(modDir, ModSigning.SignatureName);
            var signerPath = Path.Combine(modDir, ModSigning.SignerKeyName);
            if (!File.Exists(ascPath)) return $"{ModSigning.SignatureName} is missing from the mod folder; reinstall the mod";
            if (!File.Exists(signerPath)) return $"{ModSigning.SignerKeyName} is missing from the mod folder; reinstall the mod";

            ModSigning.Verification verification;
            try
            {
                verification = ModSigning.Verify(
                    checksums, await File.ReadAllTextAsync(ascPath), await File.ReadAllTextAsync(signerPath), trust!);
            }
            catch (Exception ex)
            {
                return $"the signature over {ModSigning.ChecksumsName} could not be read: {ex.Message}";
            }

            switch (verification.Trust)
            {
                case ModSigning.Trust.Invalid:
                    return $"the signature over {ModSigning.ChecksumsName} is invalid: {verification.Problem}";
                case ModSigning.Trust.Unsigned:
                    return $"{ModSigning.SignatureName} is missing from the mod folder; reinstall the mod";
                case ModSigning.Trust.Revoked:
                    return "the signing key has been revoked";
            }

            // Trusted keys are fine; an untrusted key is only fine when it is the very key
            // the user accepted at install time — otherwise the signature was replaced.
            if (verification.Key != null && !string.Equals(ModSigning.Fingerprint(verification.Key), mod.Signer, StringComparison.OrdinalIgnoreCase))
                return $"it was installed signed by [{mod.Signer}] but is now signed by {ModSigning.Describe(verification.Key)}; reinstall the mod";
        }

        // Every package file must still hash to its CHECKSUMS entry. Hashing is what makes launch
        // slow for big mods and the entries are independent, so hash them in parallel and report
        // the first problem in CHECKSUMS order.
        var checksumMap = ParseChecksumMap(checksums);
        var entries = checksums
            .Split('\n')
            .Select(entry => entry.Split(' ', StringSplitOptions.RemoveEmptyEntries))
            .Where(parts => parts.Length >= 2)
            .Select(parts => (Path: string.Join(' ', parts.Skip(1)).TrimStart('*').Trim(), Expected: parts[0].Trim().ToLowerInvariant()))
            .ToArray();
        var problems = new string?[entries.Length];
        await Parallel.ForEachAsync(Enumerable.Range(0, entries.Length), (i, _) =>
        {
            problems[i] = VerifyInstalledHash(resolvedGame, modDir, entries[i].Path, entries[i].Expected, gameFileOwners);
            return ValueTask.CompletedTask;
        });
        foreach (var problem in problems)
        {
            if (problem != null) return problem;
        }

        return AddedFilesProblemAsync(modDir, checksumMap) ?? AddedCodeProblemAsync(modDir, checksumMap, scopedToModFiles: true);
    }

    /// <summary>Checks the on-disk copies of one CHECKSUMS entry, from its package path like `dlls/64/x.dll` or `files/<game path>`.</summary>
    private static string? VerifyInstalledHash(
        string resolvedGame, string modDir, string packagePath, string expected, IReadOnlyDictionary<string, int> gameFileOwners)
    {
        if (packagePath.StartsWith("files/", StringComparison.OrdinalIgnoreCase))
        {
            var relative = packagePath["files/".Length..];

            // The mod folder snapshot is the install state and always exists.
            var snapshotHash = HashFileOrNull(Path.Combine(modDir, "files", relative));
            if (snapshotHash == null) return $"the installed file '{packagePath}' is missing; the mod was modified after install";

            // The copy in the game folder, when exactly one enabled mod writes this file.
            // INIs are merged into the game's file by value, so only snapshots are compared there.
            if (gameFileOwners.TryGetValue(relative, out var owners) &&
                owners == 1 &&
                !relative.EndsWith(".ini", StringComparison.OrdinalIgnoreCase))
            {
                var gameHash = HashFileOrNull(Path.Combine(resolvedGame, relative));
                if (gameHash != null && !string.Equals(gameHash, snapshotHash, StringComparison.OrdinalIgnoreCase))
                    return $"the game file '{relative}' was modified after install";
            }

            return string.Equals(snapshotHash, expected, StringComparison.OrdinalIgnoreCase)
                ? null
                : $"the installed file '{packagePath}' was modified after install";
        }

        // Everything else lives verbatim in the mod folder.
        var path = Path.GetFullPath(Path.Combine(modDir, packagePath));
        var fullModDir = Path.GetFullPath(modDir);
        if (!path.StartsWith(fullModDir + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
            return $"the package file '{packagePath}' would read outside the mod folder";
        var hash = HashFileOrNull(path);
        if (hash == null) return $"the package file '{packagePath}' is missing; the mod was modified after install";
        return string.Equals(hash, expected, StringComparison.OrdinalIgnoreCase)
            ? null
            : $"the file '{packagePath}' was modified after install";
    }

    /// <summary>Files under the mod folder that the package never contained must not appear.</summary>
    private static string? AddedFilesProblemAsync(string modDir, IReadOnlyDictionary<string, string> checksumMap)
    {
        var fullModDir = Path.GetFullPath(modDir);
        foreach (var file in EnumerateFilesSafe(modDir))
        {
            var relative = Path.GetRelativePath(fullModDir, file).Replace('\\', '/');
            if (relative.StartsWith("files/", StringComparison.OrdinalIgnoreCase)) continue; // snapshots of installed game copies
            if (checksumMap.ContainsKey(relative)) continue;
            if (ModSigning.IsSealEntry(relative)) continue; // CHECKSUMS.list-of CHECKSUMS and its signature live outside the list
            return $"the file '{relative}' does not belong to the installed package; the mod was modified after install";
        }
        return null;
    }

    /// <summary>
    /// An install that never carried signature state (or CHECKSUMS at all) may not have
    /// gained code since: Lua would be run and PE images injected, unsigned.
    /// When <paramref name="scopedToModFiles"/> is set (a CHECKSUMS install), the CHECKSUMS
    /// entries were already verified to still exist; only new files are inspected here.
    /// </summary>
    private static string? AddedCodeProblemAsync(string modDir, IReadOnlyDictionary<string, string> checksumMap, bool scopedToModFiles)
    {
        foreach (var file in EnumerateFilesSafe(modDir))
        {
            var relative = Path.GetRelativePath(modDir, file).Replace('\\', '/');
            if (relative.StartsWith("files/", StringComparison.OrdinalIgnoreCase)) continue;
            if (scopedToModFiles && checksumMap.ContainsKey(relative)) continue;

            if (relative.EndsWith(".lua", StringComparison.OrdinalIgnoreCase) &&
                !relative.EndsWith(".d.lua", StringComparison.OrdinalIgnoreCase))
                return $"the Lua script '{relative}' appeared after an unsigned install; the mod was modified after install";

            if (IsPeImage(file))
                return $"the PE image '{relative}' appeared after an unsigned install; the mod was modified after install";
        }
        return null;
    }

    private static bool IsPeImage(string file)
    {
        try
        {
            using var stream = File.OpenRead(file);
            Span<byte> header = stackalloc byte[2];
            return stream.ReadAtLeast(header, 2, throwOnEndOfStream: false) == 2 && header is [(byte)'M', (byte)'Z'];
        }
        catch
        {
            // Unreadable: not loadable code either way.
            return false;
        }
    }

    private static IEnumerable<string> EnumerateFilesSafe(string dir)
    {
        try
        {
            return Directory.EnumerateFiles(dir, "*", SearchOption.AllDirectories);
        }
        catch
        {
            return [];
        }
    }

    private static string? HashFileOrNull(string path)
    {
        try
        {
            using var stream = File.OpenRead(path);
            return Convert.ToHexStringLower(SHA256.HashData(stream));
        }
        catch
        {
            return null;
        }
    }

    private static string? TryGameRelative(string resolvedGame, string file)
    {
        try
        {
            if (!Path.IsPathRooted(file)) return file.Replace('\\', '/');
            return Path.GetRelativePath(resolvedGame, file);
        }
        catch
        {
            return null;
        }
    }

    private static Dictionary<string, string> ParseChecksumMap(string checksums) =>
        checksums
            .Split(["\r\n", "\r", "\n"], StringSplitOptions.RemoveEmptyEntries)
            .Select(line => line.Split(' ', StringSplitOptions.RemoveEmptyEntries))
            .Where(parts => parts.Length >= 2)
            .ToDictionary(
                parts => string.Join(' ', parts.Skip(1)).TrimStart('*').Trim(),
                parts => parts[0].Trim().ToLowerInvariant(),
                StringComparer.OrdinalIgnoreCase);
}
