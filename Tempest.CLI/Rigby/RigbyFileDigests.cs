using System.Security.Cryptography;
using Blake3;
using Tempest.CLI.Rigby.Models;

namespace Tempest.CLI.Rigby;

/// <summary>
/// The strongest digest a manifest lists for a file, fed in pieces while the bytes are still in
/// memory, so a restore never re-reads a file it just wrote. BLAKE3 when the manifest has it,
/// SHA-256 otherwise: on the same data BLAKE3 hashes about twice as fast as SHA-256 and five
/// times as fast as the MD5 the manifest also carries, so the weaker hashes are not computed.
/// </summary>
internal sealed class RigbyFileDigests : IDisposable
{
    private readonly IncrementalHash? sha256;
    private readonly Hasher blake3;
    private readonly bool hasBlake3;

    public RigbyFileDigests(RigbyFile file)
    {
        hasBlake3 = file.Blake3Bytes is not null;
        if (hasBlake3)
            blake3 = Hasher.New();
        else
            sha256 = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
    }

    public void Append(ReadOnlySpan<byte> data)
    {
        if (hasBlake3)
            blake3.Update(data);
        else
            sha256!.AppendData(data);
    }

    /// <summary>The digest the bytes do not match, or null when they match the one the file lists.</summary>
    public string? Mismatch(RigbyFile file)
    {
        Span<byte> digest = stackalloc byte[32];
        if (hasBlake3)
        {
            blake3.Finalize(digest);
            return digest.SequenceEqual(file.Blake3Bytes!) ? null : "blake3";
        }

        sha256!.GetHashAndReset(digest);
        return digest.SequenceEqual(file.Sha256Bytes) ? null : "sha256";
    }

    public void Dispose()
    {
        sha256?.Dispose();
        if (hasBlake3)
            blake3.Dispose();
    }
}
