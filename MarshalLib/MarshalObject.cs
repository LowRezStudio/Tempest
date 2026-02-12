namespace MarshalLib;

public class MarshalObject(FieldType type, object value, MarshalFlags flags = MarshalFlags.None)
{
    public FieldType Type { get; set; } = type;
    public MarshalFlags Flags { get; set; } = flags;
    public object Value { get; set; } = value;

    public ushort GetEntryHeader(ushort param)
    {
        ushort headerType = Type switch
        {
            FieldType.Byte => 1,
            FieldType.Short => 2,
            FieldType.Int => 3,
            FieldType.Long => 4,
            FieldType.DataSet => 6,
            FieldType.Guid => 7,
            FieldType.Blob => 8,
            FieldType.String => 10,
            _ => throw new ArgumentOutOfRangeException(nameof(type), type, null)
        };

        return (ushort)((headerType << 12) | (param & 0x3F));
    }

    public MarshalObject(byte value) : this(FieldType.Byte, value)
    {
    }

    public MarshalObject(ushort value) : this(FieldType.Short, value)
    {
    }

    public MarshalObject(uint value) : this(FieldType.Int, value)
    {
    }

    public MarshalObject(ulong value) : this(FieldType.Long, value)
    {
    }

    public MarshalObject(IList<Dictionary<string, MarshalObject>> value) : this(FieldType.DataSet, value)
    {
    }

    public MarshalObject(string value, MarshalFlags flags = MarshalFlags.None) : this(FieldType.String, value, flags)
    {
    }

    public MarshalObject(byte[] value) : this(FieldType.Blob, value)
    {
    }

    public MarshalObject(Guid value) : this(FieldType.Guid, value)
    {
    }

    public MarshalObject(DateTime value) : this(FieldType.DateTime, value)
    {
    }
}
