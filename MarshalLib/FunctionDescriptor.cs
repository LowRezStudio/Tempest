namespace MarshalLib;

public class FunctionDescriptor
{
    public ushort Header { get; set; }
    public ushort Flags { get; set; }
    public uint Hash { get; set; }
    public required string Name { get; set; }
}
