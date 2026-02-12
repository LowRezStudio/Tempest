namespace MarshalLib;

public class FunctionMappings
{
    private List<FunctionDescriptor> _functions = new();

    public static FunctionMappings OpenRead(Stream stream)
    {
        var mappings = new FunctionMappings();

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

            var header = reader.ReadUInt32BigEndian();
            var name = reader.ReadCString();

            var function = new FunctionDescriptor
            {
                Header = header,
                Hash = Fnv1_32.ComputeHash(name),
                Name = name,
            };

            _functions.Add(function);
        }
    }

    public FunctionDescriptor? Get(UInt32 hash) =>
        _functions.FirstOrDefault(f => f.Hash == hash);

    public FunctionDescriptor? GetByIndex(UInt16 index) =>
        _functions.ElementAtOrDefault(index);

    public FunctionDescriptor? Get(string name) =>
        _functions.FirstOrDefault(f => f.Name == name);
}