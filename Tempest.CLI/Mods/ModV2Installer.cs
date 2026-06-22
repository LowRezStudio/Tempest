using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using Tomlyn;
using Tomlyn.Model;
using Tomlyn.Serialization;
using Org.BouncyCastle.Bcpg;
using Org.BouncyCastle.Bcpg.OpenPgp;
using Org.BouncyCastle.Crypto;
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

        mDMEajkdDhYJKwYBBAHaRw8BAQdAfKiZ3VNxDLk6EWlUUQIZFQUFyRYkW4w9r4e7
        Et59G7e0HEt5aXJvIDxLeWlyb0Bwcm90b25tYWlsLmNvbT6IkwQTFgoAOxYhBNqJ
        YuGJfFCnu5uMWmwG7lWWw4ZYBQJqOR0OAhsDBQsJCAcCAiICBhUKCQgLAgQWAgMB
        Ah4HAheAAAoJEGwG7lWWw4ZYr14A/0KuopQiJBoHZveK7JiThW+WiPv69jUh9tGv
        /2oihtHkAP41hh7B88VKgvrGIOx77odJ3T743iRu9okEq0mAVr7ZCrg4BGo5HQ4S
        CisGAQQBl1UBBQEBB0DZDHj26USAveTugZ38jPqmBzrY+pFVZfzGzNYvz960TAMB
        CAeIeAQYFgoAIBYhBNqJYuGJfFCnu5uMWmwG7lWWw4ZYBQJqOR0OAhsMAAoJEGwG
        7lWWw4ZYcNIA/jd/Sy+TUvMoR52y+N+kEV4pU/l+0Z2zmZvspUvaRLsiAQC3Slwi
        7iuPeUpRq+pI7e8BYuAjvSfQtMKx0NZXGVmyDQ==
        =RKLn
        -----END PGP PUBLIC KEY BLOCK-----
        """
    ];

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

            if (checksumsEntry != null)
            {
                string checksumsContent;
                using (var reader = new StreamReader(await checksumsEntry.OpenAsync(), Encoding.UTF8))
                {
                    checksumsContent = await reader.ReadToEndAsync();
                }

                var checksumLines = checksumsContent.Split(["\r\n", "\r", "\n"], StringSplitOptions.None);
                var checksumMap = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
                foreach (var line in checksumLines)
                {
                    if (string.IsNullOrWhiteSpace(line)) continue;
                    var parts = line.Split(' ', StringSplitOptions.RemoveEmptyEntries);
                    
                    if (parts.Length < 2) continue;
                    
                    var hash = parts[0].Trim().ToLowerInvariant();
                    var rel = string.Join(" ", parts.Skip(1)).TrimStart('*').Trim().Replace('\\', '/');
                    checksumMap[rel] = hash;
                }

                foreach (var entry in archive.Entries)
                {
                    if (entry.FullName.EndsWith('/') || entry.FullName.EndsWith('\\'))
                        continue;

                    var relativePath = entry.FullName.Replace('\\', '/');
                    if (relativePath == "CHECKSUMS" || relativePath == "CHECKSUMS.asc")
                        continue;

                    if (!checksumMap.TryGetValue(relativePath, out var expectedHash))
                    {
                        throw new Exception($"File '{relativePath}' is not present in CHECKSUMS.");
                    }

                    string actualHash;
                    await using (var stream = await entry.OpenAsync())
                    using (var sha256 = SHA256.Create())
                    {
                        var hashBytes = await sha256.ComputeHashAsync(stream);
                        actualHash = Convert.ToHexString(hashBytes).ToLowerInvariant();
                    }

                    if (actualHash != expectedHash)
                    {
                        throw new Exception($"File '{relativePath}' checksum mismatch! Expected: {expectedHash}, Actual: {actualHash}");
                    }
                }

                if (ascEntry != null)
                {
                    string ascContent;
                    using (var reader = new StreamReader(await ascEntry.OpenAsync(), Encoding.UTF8))
                    {
                        ascContent = await reader.ReadToEndAsync();
                    }

                    var keysDir = TempestPathUtility.GetLocalKeysDirectory(resolvedGame);
                    var globalKeysDir = TempestPathUtility.GetGlobalKeysDirectory();
                    var signatureVerified = OfficialKeys.Any(key => VerifyData(checksumsContent, ascContent, key));

                    if (!signatureVerified)
                    {
                        var pubKeys = new List<string>();
                        if (Directory.Exists(keysDir))
                        {
                            pubKeys.AddRange(Directory.GetFiles(keysDir, "*.pub"));
                        }
                        if (Directory.Exists(globalKeysDir))
                        {
                            pubKeys.AddRange(Directory.GetFiles(globalKeysDir, "*.pub"));
                        }

                        foreach (var keyFile in pubKeys)
                        {
                            var pubKeyPem = await File.ReadAllTextAsync(keyFile);
                            
                            if (!VerifyData(checksumsContent, ascContent, pubKeyPem)) continue;
                            
                            signatureVerified = true;
                            break;
                        }
                    }

                    if (!signatureVerified)
                    {
                        await Console.Error.WriteLineAsync("Warning: Mod has an invalid or untrusted signature.");
                    }

                    if (signatureVerified)
                    {
                        isSignedAndVerified = true;
                    }
                }
                else
                {
                    await Console.Error.WriteLineAsync("Warning: Installing an unsigned mod (missing CHECKSUMS.asc).");
                }
            }
            else
            {
                await Console.Error.WriteLineAsync("Warning: Installing an unsigned mod (missing CHECKSUMS).");
            }

            if (!isSignedAndVerified && !allowUnsigned)
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
                var relativePath = entry.FullName.Replace('\\', '/');
                if (relativePath.EndsWith('/') || string.IsNullOrEmpty(entry.Name))
                {
                    continue;
                }

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
                var relativePath = entry.FullName.Replace('\\', '/');

                if (relativePath.EndsWith('/') || string.IsNullOrEmpty(entry.Name))
                {
                    continue;
                }

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

                             var iniBackupPath = TempestPathUtility.GetLocalV2IniBackupPath(resolvedGame, modId, relativeInFiles);
                            var iniBackupDir = Path.GetDirectoryName(iniBackupPath);
                            if (iniBackupDir != null) Directory.CreateDirectory(iniBackupDir);

                            var backupLines = new List<IniLine>();

                            string? currentModSection = null;
                            foreach (var line in modIniLines)
                            {
                                if (line.Type == IniLineType.Section)
                                {
                                    currentModSection = line.SectionName;
                                }
                                else if (line.Type == IniLineType.Entry && currentModSection != null && line.Key != null)
                                {
                                    var existingEntry = line.Prefix == "+" || line.Prefix == "-" ? null : gameIniLines.FirstOrDefault(l =>
                                        l.Type == IniLineType.Entry &&
                                        string.Equals(l.SectionName, currentModSection, StringComparison.OrdinalIgnoreCase) &&
                                        string.Equals(l.Key, line.Key, StringComparison.OrdinalIgnoreCase));

                                    if (existingEntry != null)
                                    {
                                        IniPatcher.ApplyPatch(backupLines, currentModSection, existingEntry.Prefix ?? "", existingEntry.Key ?? "", existingEntry.Value ?? "");
                                        existingEntry.Value = line.Value;
                                        existingEntry.Prefix = line.Prefix;
                                        existingEntry.Raw = (line.Prefix ?? "") + line.Key + "=" + line.Value;
                                    }
                                    else
                                    {
                                        IniPatcher.ApplyPatch(gameIniLines, currentModSection, line.Prefix ?? "", line.Key, line.Value ?? "");
                                    }
                                }
                            }

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

                string? currentModSection = null;
                foreach (var line in modIniLines)
                {
                    switch (line.Type)
                    {
                        case IniLineType.Section:
                            currentModSection = line.SectionName;
                            break;
                        case IniLineType.Entry when currentModSection != null && line.Key != null:
                        {
                            var backupEntry = backupLines.FirstOrDefault(l =>
                                l.Type == IniLineType.Entry &&
                                string.Equals(l.SectionName, currentModSection, StringComparison.OrdinalIgnoreCase) &&
                                string.Equals(l.Key, line.Key, StringComparison.OrdinalIgnoreCase));

                            if (backupEntry != null)
                            {
                                var gameEntry = gameIniLines.FirstOrDefault(l =>
                                    l.Type == IniLineType.Entry &&
                                    string.Equals(l.SectionName, currentModSection, StringComparison.OrdinalIgnoreCase) &&
                                    string.Equals(l.Key, line.Key, StringComparison.OrdinalIgnoreCase));

                                if (gameEntry != null)
                                {
                                    gameEntry.Value = backupEntry.Value;
                                    gameEntry.Prefix = backupEntry.Prefix;
                                    gameEntry.Raw = (backupEntry.Prefix ?? "") + backupEntry.Key + "=" + backupEntry.Value;
                                }
                            }
                            else
                            {
                                if (line.Prefix == "+")
                                {
                                    gameIniLines.RemoveAll(l =>
                                        l.Type == IniLineType.Entry &&
                                        string.Equals(l.SectionName, currentModSection, StringComparison.OrdinalIgnoreCase) &&
                                        string.Equals(l.Key, line.Key, StringComparison.OrdinalIgnoreCase) &&
                                        (string.Equals(l.Value, line.Value, StringComparison.OrdinalIgnoreCase) ||
                                         string.Equals(l.Value, line.Value + " ; tempest", StringComparison.OrdinalIgnoreCase)));
                                }
                                else if (line.Prefix != "-")
                                {
                                    gameIniLines.RemoveAll(l =>
                                        l.Type == IniLineType.Entry &&
                                        string.Equals(l.SectionName, currentModSection, StringComparison.OrdinalIgnoreCase) &&
                                        string.Equals(l.Key, line.Key, StringComparison.OrdinalIgnoreCase));
                                }
                            }

                            break;
                        }
                    }
                }

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

            var sigBlock = sigLines.Count > 0 ? string.Join("\n", sigLines) : ascContent;

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