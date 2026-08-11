using System.Buffers.Binary;
using System.Diagnostics.CodeAnalysis;
using System.Text;

namespace MarshalLib;

public class FunctionMappings
{
    private readonly List<FunctionDescriptor> _functions = [];

    public static FunctionMappings OpenRead(Stream stream)
    {
        var mappings = new FunctionMappings();

        mappings.Read(stream);

        return mappings;
    }

    /// <summary>
    /// Reads the game's embedded function token table (decompressed). Two layouts
    /// exist and are auto-detected:
    /// <code>
    /// legacy: u16  legacy function index (big-endian, the value used on the wire)
    ///         u16  flags (big-endian)
    ///         name null-terminated ASCII
    /// modern: the same four header bytes, then a u32 FNV-1 hash of the name
    ///         (big-endian) before the name
    /// </code>
    /// The hash is computed from the name when the layout stores none.
    /// </summary>
    /// <remarks>
    /// The wire index is the entry's position in the game's <c>CMarshal::s_pFunctions</c>
    /// table (e.g. 22 = GET_DATA_ASSEMBLY); the first u16 of each raw blob entry is a
    /// binary-search permutation key, not the index.
    /// </remarks>
    public void Read(Stream stream)
    {
        using var reader = new BinaryReader(stream, Encoding.UTF8, leaveOpen: true);
        var data = reader.ReadBytes((int)(stream.Length - stream.Position));

        // The modern layout stores the FNV-1 hash before the name; verify it
        // against the name to distinguish it from the legacy layout.
        var modern = data.Length > 12
            && Fnv1_32.ComputeHash(GetString(data, 8)) == BinaryPrimitives.ReadUInt32BigEndian(data.AsSpan(4, 4));

        var pos = 0;

        while (pos < data.Length)
        {
            var header = BinaryPrimitives.ReadUInt16BigEndian(data.AsSpan(pos));
            var flags = BinaryPrimitives.ReadUInt16BigEndian(data.AsSpan(pos + 2));

            var nameOffset = pos + (modern ? 8 : 4);
            var end = Array.IndexOf(data, (byte)0, nameOffset);
            if (end < 0)
                break;

            var name = Encoding.UTF8.GetString(data, nameOffset, end - nameOffset);
            var hash = modern ? BinaryPrimitives.ReadUInt32BigEndian(data.AsSpan(pos + 4)) : Fnv1_32.ComputeHash(name);

            _functions.Add(new FunctionDescriptor
            {
                Header = header,
                Flags = flags,
                Hash = hash,
                Name = name,
            });

            pos = end + 1;
        }
    }

    public FunctionDescriptor? Get(UInt32 hash) =>
        _functions.FirstOrDefault(f => f.Hash == hash);

    /// <summary>Resolves a function by its legacy wire index (the first u16 of its table entry).</summary>
    public FunctionDescriptor? GetByIndex(UInt16 index) =>
        _functions.FirstOrDefault(f => f.Header == index);

    public FunctionDescriptor? Get(string name) =>
        _functions.FirstOrDefault(f => f.Name == name);

    public bool TryGetIndex(string name, [NotNullWhen(true)] out ushort? index)
    {
        var function = _functions.FirstOrDefault(f => f.Name == name);

        if (function != null)
        {
            index = function.Header;
            return true;
        }

        index = null;
        return false;
    }

    private static string GetString(byte[] data, int offset)
    {
        var end = Array.IndexOf(data, (byte)0, offset);
        return end < 0 ? string.Empty : Encoding.UTF8.GetString(data, offset, end - offset);
    }
}
