using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using Org.BouncyCastle.Bcpg;
using Org.BouncyCastle.Bcpg.OpenPgp;
using Org.BouncyCastle.Crypto.Parameters;
using Org.BouncyCastle.Math;
using Org.BouncyCastle.Security;

namespace Tempest.CLI.Mods;

/// <summary>
/// OpenPGP signing of .tempest packages: <c>CHECKSUMS</c> lists "&lt;sha256&gt; &lt;path&gt;" for every
/// other file of the package, <c>CHECKSUMS.asc</c> is a detached signature over it and
/// <c>SIGNER.pub</c> (itself listed in CHECKSUMS) carries the signer's public key, so a signature by a
/// key that is not trusted can still be checked and shown. Which keys are trusted: <see cref="TrustStore"/>.
/// </summary>
public static class ModSigning
{
    public const string ChecksumsName = "CHECKSUMS";
    public const string SignatureName = "CHECKSUMS.asc";
    public const string SignerKeyName = "SIGNER.pub";

    public enum Trust
    {
        /// <summary>No CHECKSUMS or no CHECKSUMS.asc.</summary>
        Unsigned,
        /// <summary>Unreadable signature, signer key missing from the package, or a signature that does not match.</summary>
        Invalid,
        /// <summary>Valid signature by a key no trusted source lists.</summary>
        Untrusted,
        /// <summary>Signed by a key on the untrusted list.</summary>
        Revoked,
        Trusted,
    }

    /// <param name="KeyId">Long key ID named by the signature, when there is a readable one.</param>
    /// <param name="Key">The key the signature verified against.</param>
    /// <param name="Problem">Why an <see cref="Trust.Invalid"/> signature is invalid.</param>
    public sealed record Verification(Trust Trust, string? KeyId, PgpPublicKey? Key, string? Problem = null)
    {
        /// <summary>"user id [fingerprint]", or the bare key ID when the key is unknown.</summary>
        public string Signer => Key != null ? Describe(Key) : KeyId ?? "an unknown key";
    }

    public static string Fingerprint(PgpPublicKey key) => Convert.ToHexString(key.GetFingerprint());

    public static string Describe(PgpPublicKey key) =>
        $"{key.GetUserIds().FirstOrDefault() ?? "(no user id)"} [{Fingerprint(key)}]";

    /// <param name="signerKey">The package's SIGNER.pub, used when no trusted source has the key.</param>
    public static Verification Verify(string checksums, string? asc, string? signerKey, TrustStore trust)
    {
        if (asc == null) return new(Trust.Unsigned, null, null);

        var sig = ReadSignature(asc);
        if (sig == null) return new(Trust.Invalid, null, null, "CHECKSUMS.asc is not a readable signature");

        var keyId = sig.KeyId.ToString("X16");
        var trustedKey = trust.FindTrustedKey(sig.KeyId);
        var key = trustedKey ?? (signerKey == null ? null : FindPublicKey(signerKey, sig.KeyId));
        if (key == null)
        {
            return trust.IsRevoked(keyId)
                ? new(Trust.Revoked, keyId, null)
                : new(Trust.Invalid, keyId, null, $"the package does not include the public key {keyId} it was signed with");
        }
        if (!Matches(sig, key, checksums))
            return new(Trust.Invalid, keyId, key, "the signature does not match CHECKSUMS");
        if (trust.IsRevoked(Fingerprint(key)))
            return new(Trust.Revoked, keyId, key);
        return new(trustedKey != null ? Trust.Trusted : Trust.Untrusted, keyId, key);
    }

    /// <summary>Checks a detached signature over a text file (the published key lists).</summary>
    public static bool VerifyText(string text, string asc, PgpPublicKey key)
    {
        var sig = ReadSignature(asc);
        return sig != null && sig.KeyId == key.KeyId && Matches(sig, key, text);
    }

    /// <summary>
    /// Rewrites <c>CHECKSUMS</c> for every file of <paramref name="archive"/> and, given a key,
    /// <c>SIGNER.pub</c> and <c>CHECKSUMS.asc</c>.
    /// </summary>
    public static void Seal(ZipArchive archive, PgpSecretKey? key)
    {
        foreach (var stale in archive.Entries.Where(e => IsSealEntry(e.FullName) || e.FullName.Replace('\\', '/') == SignerKeyName).ToList())
            stale.Delete();

        if (key != null)
            WriteEntry(archive, SignerKeyName, Armor(key.PublicKey.Encode));

        var files = archive.Entries
            .Where(e => !ModV2Installer.IsDirectoryEntry(e))
            .ToDictionary(e => e.FullName.Replace('\\', '/'), StringComparer.OrdinalIgnoreCase);
        var lines = files.Keys.Order(StringComparer.Ordinal).Select(path =>
        {
            using var content = files[path].Open();
            return $"{Convert.ToHexStringLower(SHA256.HashData(content))} {path}";
        });
        var checksums = string.Join("\n", lines);

        WriteEntry(archive, ChecksumsName, checksums);
        if (key != null)
            WriteEntry(archive, SignatureName, SignText(checksums, key));
    }

    public static bool IsSealEntry(string path) => path.Replace('\\', '/') is ChecksumsName or SignatureName;

    public static PgpSecretKey ReadSecretKey(string armored)
    {
        using var input = PgpUtilities.GetDecoderStream(new MemoryStream(Encoding.UTF8.GetBytes(armored)));
        return new PgpSecretKeyRingBundle(input).GetKeyRings()
            .SelectMany(ring => ring.GetSecretKeys())
            .FirstOrDefault(k => k.IsSigningKey)
            ?? throw new Exception("No signing key found in private key file.");
    }

    public static PgpPublicKey ReadPublicKey(string armored)
    {
        using var input = PgpUtilities.GetDecoderStream(new MemoryStream(Encoding.UTF8.GetBytes(armored)));
        return new PgpPublicKeyRingBundle(input).GetKeyRings()
            .SelectMany(ring => ring.GetPublicKeys())
            .FirstOrDefault()
            ?? throw new Exception("No public key found in key file.");
    }

    /// <summary>Writes an armored RSA 2048 keypair without a passphrase; refuses to overwrite either file.</summary>
    public static PgpPublicKey GenerateKeyPair(string userId, string privateKeyPath, string publicKeyPath)
    {
        foreach (var path in new[] { privateKeyPath, publicKeyPath })
            if (File.Exists(path)) throw new Exception($"{path} already exists.");

        var kpg = GeneratorUtilities.GetKeyPairGenerator("RSA");
        kpg.Init(new RsaKeyGenerationParameters(BigInteger.ValueOf(0x10001), new SecureRandom(), 2048, 12));
        var kp = kpg.GenerateKeyPair();

        var secretKey = new PgpSecretKey(
            PgpSignature.DefaultCertification,
            PublicKeyAlgorithmTag.RsaGeneral,
            kp.Public,
            kp.Private,
            DateTime.UtcNow,
            userId,
            SymmetricKeyAlgorithmTag.Cast5,
            [],
            null,
            null,
            new SecureRandom());

        var dir = Path.GetDirectoryName(Path.GetFullPath(privateKeyPath));
        if (dir != null) Directory.CreateDirectory(dir);
        using (var secretOut = File.Create(privateKeyPath))
        {
            if (!OperatingSystem.IsWindows())
                File.SetUnixFileMode(privateKeyPath, UnixFileMode.UserRead | UnixFileMode.UserWrite);
            using var armoredOut = new ArmoredOutputStream(secretOut);
            secretKey.Encode(armoredOut);
        }

        using (var publicOut = File.Create(publicKeyPath))
        using (var armoredOut = new ArmoredOutputStream(publicOut))
        {
            secretKey.PublicKey.Encode(armoredOut);
        }
        return secretKey.PublicKey;
    }

    /// <summary>Armored detached signature over <paramref name="text"/> (line endings normalised to LF).</summary>
    public static string SignText(string text, PgpSecretKey key)
    {
        var generator = new PgpSignatureGenerator(key.PublicKey.Algorithm, HashAlgorithmTag.Sha256);
        generator.InitSign(PgpSignature.BinaryDocument, key.ExtractPrivateKey([]));
        var userId = key.PublicKey.GetUserIds().FirstOrDefault();
        if (userId != null)
        {
            var subpackets = new PgpSignatureSubpacketGenerator();
            subpackets.AddSignerUserId(isCritical: false, userId);
            generator.SetHashedSubpackets(subpackets.Generate());
        }
        generator.Update(Encoding.UTF8.GetBytes(NormalizeLineEndings(text)));
        return Armor(generator.Generate().Encode);
    }

    private static string Armor(Action<Stream> encode)
    {
        using var output = new MemoryStream();
        using (var armored = new ArmoredOutputStream(output))
        {
            encode(armored);
        }
        return Encoding.UTF8.GetString(output.ToArray());
    }

    private static bool Matches(PgpSignature sig, PgpPublicKey key, string checksums)
    {
        try
        {
            sig.InitVerify(key);
            sig.Update(Encoding.UTF8.GetBytes(NormalizeLineEndings(checksums)));
            return sig.Verify();
        }
        catch (PgpException)
        {
            return false;
        }
    }

    private static PgpSignature? ReadSignature(string asc)
    {
        // Some signatures were written without the armor footer; restore it so the decoder accepts them.
        var start = asc.IndexOf("-----BEGIN PGP SIGNATURE-----", StringComparison.Ordinal);
        var block = start >= 0 ? asc[start..] : asc;
        if (!block.Contains("-----END PGP SIGNATURE-----"))
            block = block.TrimEnd() + "\n-----END PGP SIGNATURE-----\n";

        try
        {
            using var input = PgpUtilities.GetDecoderStream(new MemoryStream(Encoding.UTF8.GetBytes(block)));
            var factory = new PgpObjectFactory(input);
            for (var obj = factory.NextPgpObject(); obj != null; obj = factory.NextPgpObject())
            {
                if (obj is PgpSignatureList { Count: > 0 } list)
                    return list[0];
            }
        }
        catch (Exception)
        {
            // not a signature
        }
        return null;
    }

    public static PgpPublicKey? FindPublicKey(string armored, long keyId)
    {
        try
        {
            using var input = PgpUtilities.GetDecoderStream(new MemoryStream(Encoding.UTF8.GetBytes(armored)));
            return new PgpPublicKeyRingBundle(input).GetPublicKey(keyId);
        }
        catch (Exception)
        {
            return null; // not a public key block
        }
    }

    private static string NormalizeLineEndings(string text) => text.Replace("\r\n", "\n");

    private static void WriteEntry(ZipArchive archive, string name, string content)
    {
        using var writer = new StreamWriter(archive.CreateEntry(name).Open(), new UTF8Encoding(false));
        writer.Write(content);
    }
}
