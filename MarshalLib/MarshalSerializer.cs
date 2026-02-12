using System.Text;

namespace MarshalLib;

public static class MarshalSerializer
{
    public static MarshalFunction DeserializeFunction(Stream stream, MarshalSerializerOptions options)
    {
        var reader = new BinaryReader(stream);
        var packet = new MarshalFunction();

        FunctionDescriptor? function;

        packet.Version = options.Version;

        if (options.Version == MarshalSerializerVersion.Modern)
        {
            var zeroByte = reader.ReadByte();
            if (zeroByte != 0x0)
                throw new Exception($"Zero byte is not 0x00: {zeroByte}");

            var functionHash = reader.ReadUInt32();
            function = options.FunctionMappings.Get(functionHash);

            packet.Function = functionHash;
        }
        else
        {
            var functionIndex = reader.ReadUInt16();
            function = options.FunctionMappings.GetByIndex(functionIndex);

            packet.Function = functionIndex;
        }

        if (function != null)
        {
            packet.FunctionName = function.Name;
        }

        var fieldCount = reader.ReadUInt16();
        var i = 0;

        while (i < fieldCount)
        {
            var field = Deserialize(stream, options);

            foreach (var entry in field)
            {
                packet.Rows[entry.Key] = entry.Value;
            }

            i += field.Count;
        }

        return packet;
    }

    public static Dictionary<string, MarshalObject> Deserialize(Stream stream, MarshalSerializerOptions options)
    {
        var result = new Dictionary<string, MarshalObject>();
        var reader = new BinaryReader(stream);

        var entryHeader = reader.ReadUInt16();

        var headerType = entryHeader >> 12;
        var headerParam = entryHeader & 0x3F;

        switch (headerType)
        {
            case 1 or 2 or 3 or 4: // Integers
                {
                    var indexes = new ushort[headerParam];

                    for (var i = 0; i < headerParam; i++)
                    {
                        indexes[i] = reader.ReadUInt16();
                    }

                    foreach (var i in indexes)
                    {
                        var field = options.FieldMappings.Get(i);

                        if (field == null) throw new Exception($"Field (index: {i}) not found");

                        var value = headerType switch
                        {
                            1 => new MarshalObject(reader.ReadByte()),
                            2 => new MarshalObject(reader.ReadUInt16()),
                            3 => new MarshalObject(reader.ReadUInt32()),
                            4 => new MarshalObject(reader.ReadUInt64()),
                            _ => throw new Exception("Invalid header type")
                        };

                        result[field.Name] = value;
                    }

                    break;
                }
            case 5: // String
                {
                    var fieldIndex = reader.ReadUInt16();
                    var field = options.FieldMappings.Get(fieldIndex);

                    if (field == null) throw new Exception($"Field (index: {fieldIndex}) not found");

                    var length = reader.ReadUInt16();
                    var isUtf16 = (length & 0x8000) != 0;

                    if (isUtf16)
                    {
                        length &= 0x7FFF;

                        if (length == 0)
                        {
                            result[field.Name] = new MarshalObject("");
                            break;
                        }

                        var byteLength = length * 2;
                        result[field.Name] = new MarshalObject(
                            Encoding.Unicode.GetString(reader.ReadBytes(byteLength)),
                            MarshalFlags.Utf16);
                        break;
                    }

                    if (length == 0)
                    {
                        result[field.Name] = new MarshalObject("");
                        break;
                    }

                    result[field.Name] = new MarshalObject(Encoding.UTF8.GetString(reader.ReadBytes(length)));

                    break;
                }
            case 6: // DataSet
                {
                    var fieldIndex = reader.ReadUInt16();
                    var field = options.FieldMappings.Get(fieldIndex);

                    if (field == null) throw new Exception($"Field (index: {fieldIndex}) not found");

                    var rowCount16 = reader.ReadUInt16();
                    var rowCount = rowCount16 == ushort.MaxValue ? reader.ReadUInt32() : rowCount16;

                    var rows = new List<Dictionary<string, MarshalObject>>((int)rowCount);

                    for (var i = 0; i < rowCount; i++)
                    {
                        var row = new Dictionary<string, MarshalObject>();
                        var entryCount = reader.ReadUInt16();

                        var j = 0;

                        while (j < entryCount)
                        {
                            var entry = Deserialize(stream, options);

                            foreach (var (key, value) in entry)
                            {
                                row[key] = value;
                            }

                            j += entry.Count;
                        }

                        rows.Add(row);
                    }

                    result[field.Name] = new MarshalObject(rows);

                    break;
                }
            case 7: // GUID
                {
                    var fieldIndex = reader.ReadUInt16();
                    var field = options.FieldMappings.Get(fieldIndex);

                    if (field == null) throw new Exception($"Field (index: {fieldIndex}) not found");

                    if (headerParam != 1)
                    {
                        var length = reader.ReadUInt16();

                        if (length == 0) throw new Exception("GUID Length is zero");
                    }

                    var guid = new Guid(reader.ReadBytes(16));

                    result[field.Name] = new MarshalObject(guid);

                    break;
                }
            case 8: // Blob
                {
                    var fieldIndex = reader.ReadUInt16();
                    var field = options.FieldMappings.Get(fieldIndex);

                    if (field == null) throw new Exception($"Field (index: {fieldIndex}) not found");

                    var length = reader.ReadUInt16();

                    result[field.Name] = new MarshalObject(reader.ReadBytes(length));

                    break;
                }
            case 9:
                {
                    var fieldIndex = reader.ReadUInt16();
                    var field = options.FieldMappings.Get(fieldIndex);

                    if (field == null) throw new Exception($"Field (index: {fieldIndex}) not found");

                    var length = headerParam * 2;
                    var bytes = reader.ReadBytes(length);
                    var str = Encoding.Unicode.GetString(bytes);

                    result[field.Name] = new MarshalObject(str, MarshalFlags.Utf16);

                    break;
                }
            case 10: // String
                {
                    var fieldIndex = reader.ReadUInt16();
                    var length = reader.ReadUInt16();

                    var isUtf32 = headerParam == 3;
                    var field = options.FieldMappings.Get(fieldIndex);

                    if (field == null) throw new Exception($"Field (index: {fieldIndex}) not found");

                    var slice = reader.ReadBytes(isUtf32 ? length * 4 : length * 2);
                    var flags = isUtf32 ? MarshalFlags.Utf32 : MarshalFlags.None;
                    var value = isUtf32 ? Encoding.UTF32.GetString(slice) : Encoding.Unicode.GetString(slice);

                    result[field.Name] = new MarshalObject(value, flags);

                    break;
                }
            default:
                throw new Exception($"Invalid header type: {headerType}");
        }

        return result;
    }

    public static void SerializeFunction(Stream stream, MarshalFunction packet, MarshalSerializerOptions options)
    {
        var writer = new BinaryWriter(stream);

        if (options.Version == MarshalSerializerVersion.Legacy)
        {
            ushort functionIndex;

            if (packet.FunctionName != null && options.FunctionMappings.TryGetIndex(packet.FunctionName, out functionIndex))
            {
            }
            else if (packet.Function != 0 && options.FunctionMappings.TryGetIndex(packet.Function, out functionIndex))
            {
            }
            else if (packet.Function != 0 && packet.Function <= ushort.MaxValue)
            {
                functionIndex = (ushort)packet.Function;
            }
            else
            {
                throw new Exception("Function index or name could not be resolved");
            }

            writer.Write(functionIndex);
            writer.Write((ushort)packet.Rows.Count);
        }
        else
        {
            var functionHash = packet.Function;

            if (functionHash == 0)
            {
                if (packet.FunctionName == null)
                    throw new Exception("Function hash and name are both null");

                var function = options.FunctionMappings.Get(packet.FunctionName);

                if (function == null)
                    throw new Exception($"Function (name: {packet.FunctionName}) not found");

                functionHash = function.Hash;
            }

            writer.Write((byte)0);
            writer.Write(functionHash);
            writer.Write((ushort)packet.Rows.Count);
        }

        foreach (var (key, value) in packet.Rows)
        {
            Serialize(stream, key, value, options);
        }
    }

    public static void Serialize(Stream stream, string field, MarshalObject marshalObject,
        MarshalSerializerOptions options)
    {
        if (!options.FieldMappings.TryGetIndex(field, out var index))
            throw new Exception($"Field (name: {field}) not found");

        Serialize(stream, index, marshalObject, options);
    }

    public static void Serialize(Stream stream, ushort fieldIndex, MarshalObject marshalObject, MarshalSerializerOptions options)
    {
        var writer = new BinaryWriter(stream);

        switch (marshalObject.Type)
        {
            case FieldType.Byte or FieldType.Short or FieldType.Int or FieldType.Long or FieldType.Guid:
                {
                    writer.Write(marshalObject.GetEntryHeader(1));
                    writer.Write(fieldIndex);

                    switch (marshalObject.Type)
                    {
                        case FieldType.Byte:
                            writer.Write((byte)marshalObject.Value);
                            break;
                        case FieldType.Short:
                            writer.Write((ushort)marshalObject.Value);
                            break;
                        case FieldType.Int:
                            writer.Write((uint)marshalObject.Value);
                            break;
                        case FieldType.Long:
                            writer.Write((ulong)marshalObject.Value);
                            break;
                        case FieldType.Guid:
                            writer.Write(((Guid)marshalObject.Value).ToByteArray());
                            break;
                    }

                    break;
                }
            case FieldType.DataSet:
                {
                    var value = (IList<Dictionary<string, MarshalObject>>)marshalObject.Value;

                    writer.Write(marshalObject.GetEntryHeader(1));
                    writer.Write(fieldIndex);

                    if (value.Count > ushort.MaxValue)
                    {
                        writer.Write(ushort.MaxValue);
                        writer.Write((uint)value.Count);
                    }
                    else
                    {
                        writer.Write((ushort)value.Count);
                    }

                    foreach (var row in value)
                    {
                        writer.Write((ushort)row.Count);

                        foreach (var entry in row)
                        {
                            Serialize(stream, entry.Key, entry.Value, options);
                        }
                    }

                    break;
                }
            case FieldType.Blob:
                {
                    var value = (byte[])marshalObject.Value;

                    writer.Write(marshalObject.GetEntryHeader(1));
                    writer.Write(fieldIndex);
                    writer.Write((ushort)value.Length);
                    writer.Write(value);

                    break;
                }
            case FieldType.String:
                {
                    var isUtf32 = marshalObject.Flags.HasFlag(MarshalFlags.Utf32);
                    writer.Write(marshalObject.GetEntryHeader((ushort)(isUtf32 ? 3 : 4)));

                    var value = (string)marshalObject.Value;

                    writer.Write(fieldIndex);
                    writer.Write((ushort)value.Length);
                    writer.Write(isUtf32 ? Encoding.UTF32.GetBytes(value) : Encoding.Unicode.GetBytes(value));

                    break;
                }
        }
    }
}
