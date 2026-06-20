namespace MarshalLib;

public class MarshalFunction
{
    public MarshalSerializerVersion Version { get; set; }
    public uint Function { get; set; }
    public string? FunctionName { get; set; }
    public Dictionary<string, MarshalObject> Rows { get; set; } = [];
}