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

        var fileBytes = File.ReadAllBytes(path);
        var written = new HashSet<string>(StringComparer.Ordinal);

        // The server-side MFIELD_TOKEN enum (the authoritative numbering for .dat
        // files) is only present as DWARF debug info in the Linux builds. When
        // available it wins over the gzip blob, whose tokens use the MCTS protocol
        // numbering; field types are joined from the blob by name. The MFUNCTION
        // enum (values = FNV-1 hashes in the modern era) covers binaries with no
        // gzip function blob (e.g. ShippingPC-ChaosServer).
        var fieldEnum = DwarfEnumReader.TryReadEnum(fileBytes, "MFIELD_TOKEN");
        var functionEnum = DwarfEnumReader.TryReadEnum(fileBytes, "MFUNCTION");
        byte[]? fieldsBlob = null;

        var scanPos = 0;

        while (scanPos + gzipHeader.Length <= fileBytes.Length)
        {
            var headerIndex = fileBytes.AsSpan(scanPos).IndexOf(gzipHeader);
            if (headerIndex < 0)
                break;

            headerIndex += scanPos;

            byte[] bytes;

            try
            {
                using var stream = new MemoryStream(fileBytes, headerIndex, fileBytes.Length - headerIndex);
                using var decompressionStream = new GZipStream(stream, CompressionMode.Decompress);
                using var memoryStream = new MemoryStream();
                decompressionStream.CopyTo(memoryStream);
                bytes = memoryStream.ToArray();
            }
            catch (InvalidDataException)
            {
                // False positive on the gzip magic; keep scanning.
                scanPos = headerIndex + 1;
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
                if (fieldEnum is not null)
                {
                    // The DWARF table wins; the blob only supplies field types.
                    fieldsBlob = bytes;
                    scanPos = headerIndex + 1;
                    continue;
                }

                filename = "fields";
            }
            else
            {
                // Unrelated compressed data; not a token table.
                scanPos = headerIndex + 1;
                continue;
            }

            File.WriteAllBytes(Path.Join(output, $"{filename}.dat"), bytes);
            written.Add(filename);

            Console.WriteLine($"Wrote all {bytes.Length} bytes to \"{filename}.dat\"");

            scanPos = headerIndex + 1;
        }

        // DWARF-only binaries (e.g. ShippingPC-ChaosServer) have no gzip blobs;
        // emit both tables from the enums after the scan.
        if (fieldEnum is not null)
        {
            var fields = BuildServerFieldsFile(fieldEnum, fieldsBlob is null ? null : ReadFieldTypes(fieldsBlob));
            File.WriteAllBytes(Path.Join(output, "fields.dat"), fields);
            written.Add("fields");
            Console.WriteLine($"Wrote all {fields.Length} bytes to \"fields.dat\" (server MFIELD_TOKEN enum)");
        }

        if (functionEnum is not null && !written.Contains("functions"))
        {
            var functions = BuildModernFunctionsFile(functionEnum);
            File.WriteAllBytes(Path.Join(output, "functions.dat"), functions);
            written.Add("functions");
            Console.WriteLine($"Wrote all {functions.Length} bytes to \"functions.dat\" (MFUNCTION enum)");
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

    /// <summary>
    /// Parses the extracted fields blob ({u16 token BE, u16 type BE, name}) into a
    /// name → type map, used to type the server enum entries.
    /// </summary>
    private static Dictionary<string, ushort> ReadFieldTypes(byte[] blob)
    {
        var types = new Dictionary<string, ushort>(StringComparer.Ordinal);
        var pos = 0;

        while (pos + 4 <= blob.Length)
        {
            var type = BinaryPrimitives.ReadUInt16BigEndian(blob.AsSpan(pos + 2));
            var end = Array.IndexOf(blob, (byte)0, pos + 4);
            if (end < 0)
                break;

            types[GetString(blob, pos + 4)] = type;
            pos = end + 1;
        }

        return types;
    }

    /// <summary>
    /// Builds the server fields table ({u16 token BE, u16 type BE, name}) from the
    /// DWARF MFIELD_TOKEN enum, with the MFTOK_ prefix stripped and field types
    /// joined from the client blob by name (defaulting to String).
    /// </summary>
    private static byte[] BuildServerFieldsFile((int Value, string Name)[] enumEntries, Dictionary<string, ushort>? types)
    {
        var output = new MemoryStream();

        foreach (var (value, rawName) in enumEntries)
        {
            if (value < 0 || value > ushort.MaxValue)
                continue;

            var name = rawName.StartsWith("MFTOK_", StringComparison.Ordinal) ? rawName[6..] : rawName;
            var type = types is not null && types.TryGetValue(name, out var known) ? known : (ushort)12; // default: String

            output.WriteByte((byte)(value >> 8));
            output.WriteByte((byte)value);
            output.WriteByte((byte)(type >> 8));
            output.WriteByte((byte)type);
            var nameBytes = System.Text.Encoding.UTF8.GetBytes(name);
            output.Write(nameBytes);
            output.WriteByte(0);
        }

        return output.ToArray();
    }

    /// <summary>
    /// Builds a modern functions table ({u16 header, u16 flags, u32 FNV-1 hash BE,
    /// name}) from the DWARF MFUNCTION enum, whose values are the FNV-1 hashes
    /// themselves (e.g. GET_DATA_ASSEMBLY = 0x2E2EC0E9).
    /// </summary>
    private static byte[] BuildModernFunctionsFile((int Value, string Name)[] enumEntries)
    {
        var output = new MemoryStream();

        foreach (var (value, rawName) in enumEntries)
        {
            // Values are u32 FNV-1 hashes; a negative int is just a hash with the
            // top bit set, not an invalid entry.
            var name = rawName.StartsWith("MFUNC_", StringComparison.Ordinal) ? rawName[6..] : rawName;
            var hash = unchecked((uint)value);

            output.WriteByte(0);
            output.WriteByte(0);
            output.WriteByte(0);
            output.WriteByte(0);
            output.WriteByte((byte)(hash >> 24));
            output.WriteByte((byte)(hash >> 16));
            output.WriteByte((byte)(hash >> 8));
            output.WriteByte((byte)hash);
            var nameBytes = System.Text.Encoding.UTF8.GetBytes(name);
            output.Write(nameBytes);
            output.WriteByte(0);
        }

        return output.ToArray();
    }
}
