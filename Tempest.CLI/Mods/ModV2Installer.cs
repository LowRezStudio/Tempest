using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using Tomlyn;
using Tomlyn.Model;
using Tomlyn.Serialization;
using Org.BouncyCastle.Bcpg;
using Org.BouncyCastle.Bcpg.OpenPgp;
using Org.BouncyCastle.Crypto.Parameters;
using Org.BouncyCastle.Math;
using Org.BouncyCastle.Security;

namespace Tempest.CLI.Mods;

[TomlSerializable(typeof(TomlTable))]
internal partial class ModTomlContext : TomlSerializerContext
{
}

public class ModV2Installer : IModInstaller
{
    private static readonly string[] OfficialKeys = [
        // Kyiro
        """
        -----BEGIN PGP PUBLIC KEY BLOCK-----
        Version: BouncyCastle.NET Cryptography (net6.0) v2.6.2+b4f2f6ad76

        mQENBGo76U8BCACXjyMsYNtl+CrwJM5gHEX8ZWGnGF5P9BH8FUXyNyBLhHl9FIMO
        Q82GUFgseM7sB6tpOI5ny/oM/rGnB3x083WX3iPfnEqGCE7mwewVBBZ161nORFXF
        ZSHmS9GGc49gaegXca0Zn5buO/AVf2dSX3V/DTGlM9k37PBkhLIkq9nWreF+ms7p
        LKbN6OTC91rCoG0uu+o1xufacXA2oP6HwHfjey0BHCfNJMe0XXj996fPjd6GQiPp
        vRzFduv4xD8beq0ESLmnATgGpIz9V4fuj199aQarC9UXnrK5/307ii9alnKSwMkC
        2Ft+LmgJznva5lWC/X5B72tGlo49P8qKSuYPABEBAAG0FXRlbXBlc3RAbG93cmV6
        LnN0dWRpb4kBHAQQAQIABgUCajvpTwAKCRCros/8spKO7PY7B/9aoBdneA6D2Wis
        AlGxN1JODeMLVlJRJ7/c0ctP+f79tr9QF9RmR9Q2Wp+n/f8vgkXvrOLCkCZeRiL5
        HmkA7INoTPQ1wo0hegfo1s6/0DZ5fT/9vsqh/U+m2+MGj4qeEFJl6MZIipBUrd3h
        +7A7BS9y6D6pCQD7OJDUSdLUIVpQf3mmTo+/RWRJt3Dpt5E/xvcL+hkBfe+VMhgE
        ynFRVWCVYyrEX0rx461H8AuMACBrkN8y15TGp3BhwYpV24zjSTKfgUCiw5637AQs
        vLPTkL7z765hBld34NBGBe4hadnHqNOoQgG9lu4w4tmr3iLsZHnQwv6QPeVmwyNq
        nkcUNeY9
        =dsdU
        -----END PGP PUBLIC KEY BLOCK-----
        """
    ];

    private static string NormalizeEntryPath(string fullName) => fullName.Replace('\\', '/');
    private static bool IsDirectoryEntry(ZipArchiveEntry entry) =>
        entry.FullName.EndsWith('/') || string.IsNullOrEmpty(entry.Name);

    public async Task<ModInstallResult> InstallAsync(string gamePath, string modFilePath, bool replace, bool allowUnsigned)
    {
        var resolvedGame = GameFolderResolver.Resolve(gamePath);
        var isSignedAndVerified = false;

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

            var checksumsEntry = archive.GetEntry("CHECKSUMS");
            var ascEntry = archive.GetEntry("CHECKSUMS.asc");

            var hasDlls = archive.Entries.Any(e => !IsDirectoryEntry(e) &&
                NormalizeEntryPath(e.FullName).StartsWith("dlls/", StringComparison.OrdinalIgnoreCase) &&
                Path.GetExtension(e.Name).Equals(".dll", StringComparison.OrdinalIgnoreCase));

            if (checksumsEntry != null)
            {
                string checksumsContent;
                using (var reader = new StreamReader(await checksumsEntry.OpenAsync(), Encoding.UTF8))
                {
                    checksumsContent = await reader.ReadToEndAsync();
                }

                var checksumMap = ParseChecksumMap(checksumsContent);
                await VerifyArchiveChecksumsAsync(archive, checksumMap);

                if (await VerifySignatureAsync(checksumsContent, ascEntry, resolvedGame, hasDlls))
                {
                    isSignedAndVerified = true;
                }
            }
            else if (hasDlls)
            {
                await Console.Error.WriteLineAsync("Warning: Installing an unsigned mod (missing CHECKSUMS).");
            }

            if (!isSignedAndVerified && !allowUnsigned && hasDlls)
            {
                return new ModInstallResult
                {
                    Success = false,
                    Unverified = true,
                    Message = "Installing unsigned or unverified mods is not allowed without the '--allow-unsigned' flag."
                };
            }

            var targetModDir = TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, modId);

            if (Directory.Exists(targetModDir))
            {
                if (!replace)
                {
                    return new ModInstallResult
                    {
                        Success = false,
                        Conflict = true,
                        IsModConflict = true,
                        Message = $"Mod with ID '{modId}' is already installed."
                    };
                }

                var existingMods = ModCommands.LoadMetadata(gamePath);
                var existingMod = existingMods.FirstOrDefault(m => string.Equals(m.Id, modId, StringComparison.OrdinalIgnoreCase));
                if (existingMod != null)
                {
                    await RemoveAsync(gamePath, existingMod);
                }
            }

            var metadataMods = ModCommands.LoadMetadata(gamePath);
            foreach (var entry in archive.Entries)
            {
                if (IsDirectoryEntry(entry)) continue;

                var relativePath = NormalizeEntryPath(entry.FullName);
                if (!relativePath.StartsWith("files/")) continue;

                var relativeInFiles = relativePath["files/".Length..];
                var ext = Path.GetExtension(entry.Name).ToLowerInvariant();

                if (ext == ".ini") continue;

                var destGamePath = Path.Combine(resolvedGame, relativeInFiles);
                if (!File.Exists(destGamePath)) continue;

                if (replace) continue;

                var isMod = metadataMods.Any(m => m.InstalledFiles.Any(f => string.Equals(f, destGamePath, StringComparison.OrdinalIgnoreCase)));

                return new ModInstallResult
                {
                    Success = false,
                    Conflict = true,
                    IsModConflict = isMod,
                    Message = $"File '{relativeInFiles}' already exists in destination."
                };
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
                        var destGamePath = Path.Combine(resolvedGame, relativeInFiles);
                        var destDir = Path.GetDirectoryName(destGamePath);
                        if (destDir != null) Directory.CreateDirectory(destDir);

                        if (File.Exists(destGamePath))
                        {
                            var backupPath = TempestPathUtility.GetLocalV2BackupPath(resolvedGame, relativeInFiles);
                            var backupDir = Path.GetDirectoryName(backupPath);
                            if (backupDir != null) Directory.CreateDirectory(backupDir);

                            if (File.Exists(backupPath))
                            {
                                // ponytail: backup already exists (the pristine original), do not overwrite it.
                                // Just delete the existing destination file to copy/write the new mod file.
                                File.Delete(destGamePath);
                            }
                            else
                            {
                                File.Copy(destGamePath, backupPath);
                            }
                        }

                        await using (var srcStream = await entry.OpenAsync())
                        await using (var destStream = File.Create(destGamePath))
                        {
                            await srcStream.CopyToAsync(destStream);
                        }

                        installedFiles.Add(destGamePath);
                    }
                    else
                    {
                        var destGamePath = Path.Combine(resolvedGame, relativeInFiles);
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
                    var destModPath = Path.Combine(targetModDir, relativePath);
                    var destDir = Path.GetDirectoryName(destModPath);
                    if (destDir != null) Directory.CreateDirectory(destDir);

                    await using var srcStream = await entry.OpenAsync();
                    await using var destStream = File.Create(destModPath);

                    await srcStream.CopyToAsync(destStream);
                }
            }

            var authorString = string.Join(", ", manifest.Authors.Select(a => a.Name));
            if (string.IsNullOrEmpty(authorString)) authorString = "Unknown";

            var modRecord = new ModRecord
            {
                Id = modId,
                Name = manifest.Name,
                Author = authorString,
                Authors = manifest.Authors,
                Version = "1.0.0",
                Enabled = true,
                Kind = "V2",
                OriginalPath = modFilePath,
                InstalledFiles = installedFiles,
                Readme = manifest.Readme
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

        foreach (var file in mod.InstalledFiles)
        {
            var ext = Path.GetExtension(file).ToLowerInvariant();
            if (ext == ".ini") continue;

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
            if (relativePath is "CHECKSUMS" or "CHECKSUMS.asc") continue;

            if (!checksumMap.TryGetValue(relativePath, out var expectedHash))
                throw new Exception($"File '{relativePath}' is not present in CHECKSUMS.");

            await using var stream = await entry.OpenAsync();
            using var sha256 = SHA256.Create();
            var actualHash = Convert.ToHexString(await sha256.ComputeHashAsync(stream)).ToLowerInvariant();

            if (actualHash != expectedHash)
                throw new Exception($"File '{relativePath}' checksum mismatch! Expected: {expectedHash}, Actual: {actualHash}");
        }
    }

    private static async Task<bool> VerifySignatureAsync(string checksumsContent, ZipArchiveEntry? ascEntry, string resolvedGame, bool hasDlls)
    {
        if (ascEntry == null)
        {
            if (hasDlls)
                await Console.Error.WriteLineAsync("Warning: Installing an unsigned mod (missing CHECKSUMS.asc).");
            return false;
        }

        string ascContent;
        using (var reader = new StreamReader(await ascEntry.OpenAsync(), Encoding.UTF8))
        {
            ascContent = await reader.ReadToEndAsync();
        }

        if (OfficialKeys.Any(key => VerifyData(checksumsContent, ascContent, key)))
            return true;

        var keysDir = TempestPathUtility.GetLocalKeysDirectory(resolvedGame);
        var globalKeysDir = TempestPathUtility.GetGlobalKeysDirectory();

        var pubKeys = new[] { keysDir, globalKeysDir }
            .Where(Directory.Exists)
            .SelectMany(d => Directory.GetFiles(d, "*.pub"));

        foreach (var keyFile in pubKeys)
        {
            var pubKeyPem = await File.ReadAllTextAsync(keyFile);
            if (VerifyData(checksumsContent, ascContent, pubKeyPem))
                return true;
        }

        await Console.Error.WriteLineAsync("Warning: Mod has an invalid or untrusted signature.");
        return false;
    }

    public static string SignData(string content, string privateKeyPem)
    {
        using var keyIn = new MemoryStream(Encoding.UTF8.GetBytes(privateKeyPem));
        using var decStream = PgpUtilities.GetDecoderStream(keyIn);

        var pgpSec = new PgpSecretKeyRingBundle(decStream);

        PgpSecretKey? secretKey = null;

        foreach (var kRing in pgpSec.GetKeyRings())
        {
            foreach (var k in kRing.GetSecretKeys())
            {
                if (k.IsSigningKey)
                {
                    secretKey = k;
                    break;
                }
            }
            if (secretKey != null) break;
        }

        if (secretKey == null) throw new Exception("No signing key found in private key file.");

        var pgpPrivKey = secretKey.ExtractPrivateKey("".ToCharArray());

        using var memOut = new MemoryStream();
        using var armoredOut = new ArmoredOutputStream(memOut);

        var sGen = new PgpSignatureGenerator(secretKey.PublicKey.Algorithm, HashAlgorithmTag.Sha256);

        sGen.InitSign(PgpSignature.BinaryDocument, pgpPrivKey);
        foreach (var userId in secretKey.PublicKey.GetUserIds())
        {
            var spGen = new PgpSignatureSubpacketGenerator();
            spGen.AddSignerUserId(isCritical: false, userId);
            sGen.SetHashedSubpackets(spGen.Generate());
            break;
        }

        var normalized = content.Replace("\r\n", "\n");
        var dataBytes = Encoding.UTF8.GetBytes(normalized);
        sGen.Update(dataBytes);

        sGen.Generate().Encode(armoredOut);
        armoredOut.Close();

        return Encoding.UTF8.GetString(memOut.ToArray());
    }

    public static bool VerifyData(string checksumsContent, string ascContent, string publicKeyPem)
    {
        try
        {
            using var keyIn = new MemoryStream(Encoding.UTF8.GetBytes(publicKeyPem));
            using var decKeyStream = PgpUtilities.GetDecoderStream(keyIn);

            var pgpPub = new PgpPublicKeyRingBundle(decKeyStream);

            var lines = ascContent.Split(["\r\n", "\r", "\n"], StringSplitOptions.None);
            var sigLines = new List<string>();
            var inSignature = false;

            foreach (var line in lines)
            {
                if (line.Contains("-----BEGIN PGP SIGNATURE-----"))
                {
                    inSignature = true;
                }
                if (inSignature)
                {
                    sigLines.Add(line);
                }
                if (line.Contains("-----END PGP SIGNATURE-----"))
                {
                    break;
                }
            }

            var sigBlock = sigLines.Count > 0 ? string.Join("\r\n", sigLines) : ascContent;

            if (!sigBlock.Contains("-----END PGP SIGNATURE-----"))
            {
                sigBlock = sigBlock.TrimEnd() + "\r\n-----END PGP SIGNATURE-----\r\n";
            }

            using var memIn = new MemoryStream(Encoding.UTF8.GetBytes(sigBlock));
            using var decStream = PgpUtilities.GetDecoderStream(memIn);
            var pgpFact = new PgpObjectFactory(decStream);
            var obj = pgpFact.NextPgpObject();

            PgpSignatureList? sigList = null;

            while (obj != null)
            {
                if (obj is PgpSignatureList list)
                {
                    sigList = list;
                    break;
                }
                obj = pgpFact.NextPgpObject();
            }

            if (sigList == null || sigList.Count == 0) return false;

            var sig = sigList[0];
            var pubKey = pgpPub.GetPublicKey(sig.KeyId);
            if (pubKey == null) return false;

            sig.InitVerify(pubKey);

            var normalized = checksumsContent.Replace("\r\n", "\n");
            var dataBytes = Encoding.UTF8.GetBytes(normalized);
            sig.Update(dataBytes);

            return sig.Verify();
        }
        catch
        {
            return false;
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

    public static void GeneratePgpKeyPair(string id, string privateKeyPath, string publicKeyPath)
    {
        var kpg = GeneratorUtilities.GetKeyPairGenerator("RSA");
        kpg.Init(new RsaKeyGenerationParameters(BigInteger.ValueOf(0x10001), new SecureRandom(), 2048, 12));
        var kp = kpg.GenerateKeyPair();

        var secretKey = new PgpSecretKey(
            PgpSignature.DefaultCertification,
            PublicKeyAlgorithmTag.RsaGeneral,
            kp.Public,
            kp.Private,
            DateTime.UtcNow,
            id,
            SymmetricKeyAlgorithmTag.Cast5,
            "".ToCharArray(),
            null,
            null,
            new SecureRandom()
        );

        using (var secretOut = File.Create(privateKeyPath))
        using (var armoredOut = new ArmoredOutputStream(secretOut))
        {
            secretKey.Encode(armoredOut);
        }

        using (var publicOut = File.Create(publicKeyPath))
        using (var armoredOut = new ArmoredOutputStream(publicOut))
        {
            secretKey.PublicKey.Encode(armoredOut);
        }
    }
}

public class ModManifest
{
    public string Id { get; set; } = string.Empty;
    public string Name { get; set; } = string.Empty;
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
