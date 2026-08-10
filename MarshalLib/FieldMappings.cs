using System.Diagnostics.CodeAnalysis;

namespace MarshalLib;

public class FieldMappings
{
    private readonly Dictionary<ushort, FieldDescriptor> _fields = [];
    private readonly Dictionary<string, ushort> _fieldNames = [];

    public static FieldMappings OpenRead(Stream stream)
    {
        var mappings = new FieldMappings();

        mappings.Read(stream);

        return mappings;
    }

    public void Read(Stream stream)
    {
        using var reader = new BinaryReader(stream);

        while (true)
        {
            if (reader.BaseStream.Position == reader.BaseStream.Length)
                break;

            var field = new FieldDescriptor
            {
                Header = reader.ReadUInt16BigEndian(),
                Type = (FieldType)reader.ReadUInt16BigEndian(),
                Name = reader.ReadCString()
            };

            // The header is the field token used on the wire.
            _fields[field.Header] = field;
            _fieldNames[field.Name] = field.Header;
        }
    }

    public FieldDescriptor? Get(ushort index) =>
        _fields.GetValueOrDefault(index);

    /// <summary>All known fields (the game's complete field dictionary).</summary>
    public IEnumerable<FieldDescriptor> Fields => _fields.Values;

    public bool TryGetIndex(string name, [NotNullWhen(true)] out ushort? index)
    {
        if (_fieldNames.TryGetValue(name, out var value))
        {
            index = value;
            return true;
        }

        index = null;
        return false;
    }

    public ushort GetIndex(string name) =>
        _fieldNames.GetValueOrDefault(name);
}
