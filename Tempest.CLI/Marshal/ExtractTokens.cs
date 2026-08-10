using System.Buffers.Binary;
using System.IO.Compression;
using MarshalLib;
using Tempest.CLI.Extensions;

namespace Tempest.CLI.Marshal;

internal partial class MarshalCommands
{
    /// <summary>Extracts uncompressed function and field token data from a PE</summary>
    /// <param name="path">Path of the DLL or EXE</param>
    /// <param name="output">Output directory</param>
    public void ExtractTokens(string path, string output)
    {
        Directory.CreateDirectory(output);

        // Gzip magic only; the header's XFL/OS bytes differ between builds
        // (e.g. 04 00 vs 00 03), so match on 1F 8B 08 and verify by decoding.
        byte[] gzipHeader = [0x1F, 0x8B, 0x08];

        if (!File.Exists(path))
        {
            Console.Error.WriteLine("EXE/DLL was not found");
            Environment.ExitCode = 1;
            return;
        }

        using var file = File.Open(path, FileMode.Open);
        var written = new HashSet<string>(StringComparer.Ordinal);

        long headerIndex;

        while ((headerIndex = file.IndexOfBytes(gzipHeader)) != -1)
        {
            file.Position = headerIndex;

            byte[] bytes;

            try
            {
                using var decompressionStream = new GZipStream(file, CompressionMode.Decompress, leaveOpen: true);
                using var memoryStream = new MemoryStream();
                decompressionStream.CopyTo(memoryStream);
                bytes = memoryStream.ToArray();
            }
            catch (InvalidDataException)
            {
                // False positive on the gzip magic; keep scanning.
                file.Position = headerIndex + 1;
                continue;
            }

            var filename = headerIndex.ToString();

            var helloIndex = bytes.IndexOfBytes("HELLO"u8.ToArray());
            var versionIndex = bytes.IndexOfBytes("VERSION"u8.ToArray());

            if (helloIndex != -1)
            {
                filename = "functions";
                bytes = RewriteLegacyFunctionHeaders(bytes);
            }
            else if (versionIndex != -1)
            {
                filename = "fields";
            }
            else
            {
                // Unrelated compressed data; not a token table.
                file.Position = headerIndex + 1;
                continue;
            }

            File.WriteAllBytes(Path.Join(output, $"{filename}.dat"), bytes);
            written.Add(filename);

            Console.WriteLine($"Wrote all {bytes.Length} bytes to \"{filename}.dat\"");

            file.Position = headerIndex + 1;
        }

        if (written.Count == 0)
        {
            Console.Error.WriteLine("No token data found in the provided file.");
            Environment.ExitCode = 1;
        }
        else if (!written.Contains("functions") || !written.Contains("fields"))
        {
            // A stale sibling file from a previous era must not be reused;
            // mixed-era tables make exports fail or silently misname fields.
            Console.Error.WriteLine("Incomplete token data: expected both functions.dat and fields.dat.");
            Environment.ExitCode = 1;
        }
    }

    /// <summary>
    /// The first u16 of each entry in the LEGACY function table is a
    /// binary-search permutation key (CMarshal::GetFunction sorts by name),
    /// not the wire index - the game resolves names by table POSITION.
    /// Rewrite the header to the entry position so the exported table maps
    /// wire index → name. The modern layout (u32 FNV-1 hash before the name)
    /// is left as-is: lookups are hash-based.
    /// </summary>
    private static byte[] RewriteLegacyFunctionHeaders(byte[] bytes)
    {
        // Modern layout: big-endian FNV-1 hash at offset 4 of the name at
        // offset 8 (same heuristic as FunctionMappings.Read).
        if (bytes.Length > 12
            && Fnv1_32.ComputeHash(GetString(bytes, 8)) == BinaryPrimitives.ReadUInt32BigEndian(bytes.AsSpan(4, 4)))
        {
            return bytes;
        }

        var position = 0;
        var pos = 0;

        while (pos + 4 <= bytes.Length)
        {
            var end = Array.IndexOf(bytes, (byte)0, pos + 4);
            if (end < 0 || position > ushort.MaxValue)
                break;

            BinaryPrimitives.WriteUInt16BigEndian(bytes.AsSpan(pos, 2), (ushort)position);
            pos = end + 1;
            position++;
        }

        return bytes;
    }

    private static string GetString(byte[] data, int offset)
    {
        var end = Array.IndexOf(data, (byte)0, offset);
        return end < 0 ? string.Empty : System.Text.Encoding.UTF8.GetString(data, offset, end - offset);
    }
}
