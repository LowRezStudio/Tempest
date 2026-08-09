using System.Diagnostics.CodeAnalysis;

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
    /// Reads the game's embedded function token table (s_pbyFunctionData, decompressed).
    /// Entry layout, verified against CMarshal::PlatInitialize:
    /// <code>
    /// u16  legacy index / sort index (big-endian, ignored by the game at runtime)
    /// u16  flags (big-endian)
    /// u32  FNV-1 hash of the function name (big-endian)
    /// name null-terminated ASCII
    /// </code>
    /// </summary>
    public void Read(Stream stream)
    {
        using var reader = new BinaryReader(stream);

        while (true)
        {
            if (reader.BaseStream.Position == reader.BaseStream.Length)
                break;

            var header = reader.ReadUInt16BigEndian();
            var flags = reader.ReadUInt16BigEndian();
            var hash = reader.ReadUInt32BigEndian();
            var name = reader.ReadCString();

            _functions.Add(new FunctionDescriptor
            {
                Header = header,
                Flags = flags,
                Hash = hash,
                Name = name,
            });
        }
    }

    public FunctionDescriptor? Get(UInt32 hash) =>
        _functions.FirstOrDefault(f => f.Hash == hash);

    public FunctionDescriptor? GetByIndex(UInt16 index) =>
        _functions.ElementAtOrDefault(index);

    public FunctionDescriptor? Get(string name) =>
        _functions.FirstOrDefault(f => f.Name == name);

    public bool TryGetIndex(string name, [NotNullWhen(true)] out ushort? index)
    {
        for (var i = 0; i < _functions.Count; i++)
        {
            if (_functions[i].Name == name)
            {
                index = (ushort)i;
                return true;
            }
        }

        index = null;
        return false;
    }

    public bool TryGetIndex(uint hash, [NotNullWhen(true)] out ushort? index)
    {
        for (var i = 0; i < _functions.Count; i++)
        {
            if (_functions[i].Hash == hash)
            {
                index = (ushort)i;
                return true;
            }
        }

        index = null;
        return false;
    }
}
