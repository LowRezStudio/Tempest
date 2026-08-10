using System.Text;

namespace MarshalLib;

public static class MarshalSerializer
{
    /// <summary>Deserializes a marshal binary into a <see cref="MarshalFunction"/>.</summary>
    /// <remarks>
    /// Wire format (verified against CMarshal::Load / CMarshalRow::Load in the game binary):
    /// <code>
    /// u8   flags          (0 for ordinary marshals)
    /// u32  function hash  (FNV-1 of the function name, little-endian)
    /// u16  entry count    (0xFFFF means a u32 count follows)
    /// ... entries (see <see cref="Deserialize"/>)
    /// </code>
    /// </remarks>
    public static MarshalFunction DeserializeFunction(Stream stream, MarshalSerializerOptions options)
    {
        using var reader = new BinaryReader(stream, Encoding.UTF8, leaveOpen: true);
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
            var field = Deserialize(reader, options);

            foreach (var entry in field)
            {
                packet.Rows[entry.Key] = entry.Value;
            }

            i += field.Count;
        }

        return packet;
    }

    /// <summary>
    /// Deserializes one entry header (which may describe several consecutive fields)
    /// into a dictionary of field name to value. The dictionary preserves insertion order.
    /// </summary>
    /// <remarks>
    /// Entry header wire format (verified against CMarshalRow::LoadEntries):
    /// <code>
    /// u16 header: bits 15..12 = type, bits 5..0 = param
    ///   type 1: param field indexes (u16 each) + param byte values
    ///   type 2: param field indexes (u16 each) + param u16 values
    ///   type 3: param field indexes (u16 each) + param u32 values
    ///   type 4: param field indexes (u16 each) + param u64 values
    ///   type 5: u16 field index, u16 length (bit 15 = UTF-16), then UTF-8 bytes or UTF-16 units
    ///   type 6: u16 field index, then a row set (u16 row count, 0xFFFF = u32, then rows)
    ///   type 7: u16 field index, 16 raw GUID bytes
    ///   type 8: u16 field index, u16 size, size bytes
    ///   type 9: u16 field index, param UTF-16 units stored inline (1..3, 0 = empty)
    ///   type 10: u16 field index, u16 length, then encoded units:
    ///            param 1 = UTF-8 (length bytes), 2/3 = UTF-32 (length * 4 bytes),
    ///            4 = UTF-16 (length * 2 bytes), 5 = truncated UTF-16 (length bytes)
    /// </code>
    /// </remarks>
    public static Dictionary<string, MarshalObject> Deserialize(Stream stream, MarshalSerializerOptions options)
    {
        using var reader = new BinaryReader(stream, Encoding.UTF8, leaveOpen: true);
        return Deserialize(reader, options);
    }

    private static Dictionary<string, MarshalObject> Deserialize(BinaryReader reader, MarshalSerializerOptions options)
    {
        var result = new Dictionary<string, MarshalObject>();

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
                        var field = options.FieldMappings.Get(i) ?? throw new Exception($"Field (index: {i}) not found");
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
            case 5: // Legacy string (length high bit selects UTF-16)
                {
                    var fieldIndex = reader.ReadUInt16();
                    var field = options.FieldMappings.Get(fieldIndex) ?? throw new Exception($"Field (index: {fieldIndex}) not found");
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

                    // Unflagged legacy strings are one byte per character
                    // (the game reads them as truncated UTF-16), not UTF-8.
                    result[field.Name] = new MarshalObject(
                        Encoding.Latin1.GetString(reader.ReadBytes(length)),
                        MarshalFlags.Ascii);

                    break;
                }
            case 6: // DataSet
                {
                    var fieldIndex = reader.ReadUInt16();
                    var field = options.FieldMappings.Get(fieldIndex) ?? throw new Exception($"Field (index: {fieldIndex}) not found");
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
                            var entry = Deserialize(reader, options);

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
            case 7: // GUID: 16 raw bytes, no length prefix
                {
                    var fieldIndex = reader.ReadUInt16();
                    var field = options.FieldMappings.Get(fieldIndex) ?? throw new Exception($"Field (index: {fieldIndex}) not found");

                    var guid = new Guid(reader.ReadBytes(16));

                    result[field.Name] = new MarshalObject(guid);

                    break;
                }
            case 8: // Blob
                {
                    var fieldIndex = reader.ReadUInt16();
                    var field = options.FieldMappings.Get(fieldIndex) ?? throw new Exception($"Field (index: {fieldIndex}) not found");
                    var length = reader.ReadUInt16();

                    result[field.Name] = new MarshalObject(reader.ReadBytes(length));

                    break;
                }
            case 9: // Inline UTF-16 string, param UTF-16 units (1..3)
                {
                    var fieldIndex = reader.ReadUInt16();
                    var field = options.FieldMappings.Get(fieldIndex) ?? throw new Exception($"Field (index: {fieldIndex}) not found");
                    var length = headerParam * 2;
                    var bytes = reader.ReadBytes(length);
                    var str = Encoding.Unicode.GetString(bytes);

                    result[field.Name] = new MarshalObject(str, MarshalFlags.Utf16);

                    break;
                }
            case 10: // String with encoding in the header param
                {
                    var fieldIndex = reader.ReadUInt16();
                    var field = options.FieldMappings.Get(fieldIndex) ?? throw new Exception($"Field (index: {fieldIndex}) not found");
                    var length = reader.ReadUInt16();

                    switch (headerParam)
                    {
                        case 1: // UTF-8, length is the byte count
                            result[field.Name] = new MarshalObject(
                                Encoding.UTF8.GetString(reader.ReadBytes(length)),
                                MarshalFlags.Utf8);
                            break;
                        case 2 or 3: // UTF-32 (4-byte units)
                            result[field.Name] = new MarshalObject(
                                Encoding.UTF32.GetString(reader.ReadBytes((int)length * 4)),
                                MarshalFlags.Utf32);
                            break;
                        case 4: // UTF-16 (2-byte units)
                            result[field.Name] = new MarshalObject(
                                Encoding.Unicode.GetString(reader.ReadBytes((int)length * 2)),
                                MarshalFlags.Utf16);
                            break;
                        case 5: // Truncated UTF-16: one byte per character
                            result[field.Name] = new MarshalObject(
                                Encoding.Latin1.GetString(reader.ReadBytes(length)),
                                MarshalFlags.Ascii);
                            break;
                        default:
                            throw new Exception($"Invalid string encoding: {headerParam}");
                    }

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

            if (packet.FunctionName != null && options.FunctionMappings.TryGetIndex(packet.FunctionName, out var functionIndexFromName))
            {
                functionIndex = functionIndexFromName.Value;
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

            if (functionHash == 0 && packet.FunctionName != null)
            {
                var function = options.FunctionMappings.Get(packet.FunctionName);
                if (function != null)
                {
                    functionHash = function.Hash;
                }
            }

            // A zero hash is legal: MFUNCTION::UNKNOWN (e.g. localization files).
            writer.Write((byte)0);
            writer.Write(functionHash);
            writer.Write((ushort)packet.Rows.Count);
        }

        var rowWriter = new RowWriter(stream, options);

        foreach (var (key, value) in packet.Rows)
        {
            rowWriter.WriteEntry(key, value);
        }

        rowWriter.Flush();
    }

    public static void Serialize(Stream stream, string field, MarshalObject marshalObject,
        MarshalSerializerOptions options)
    {
        if (!options.FieldMappings.TryGetIndex(field, out var index))
            throw new Exception($"Field (name: {field}) not found");

        Serialize(stream, index.Value, marshalObject, options);
    }

    /// <summary>
    /// Serializes a single entry using the exact batching semantics of
    /// CMarshalRow::StoreEntries: integer fields accumulate into per-width caches that are
    /// flushed in width order (1, 2, 3, 4) when full (32 entries) or at the end of the row;
    /// strings, row sets, GUIDs and blobs are written inline as they are encountered.
    /// </summary>
    public static void Serialize(Stream stream, ushort fieldIndex, MarshalObject marshalObject, MarshalSerializerOptions options)
    {
        var rowWriter = new RowWriter(stream, options);
        rowWriter.WriteEntry(fieldIndex, marshalObject);
        rowWriter.Flush();
    }

    /// <summary>
    /// Replicates the exact entry writer of CMarshalRow::StoreEntries, including the
    /// value-driven width selection and the per-width batching.
    /// </summary>
    private sealed class RowWriter
    {
        private readonly BinaryWriter _writer;
        private readonly MarshalSerializerOptions _options;
        private readonly bool _legacy;

        public RowWriter(Stream stream, MarshalSerializerOptions options)
            : this(new BinaryWriter(stream), options)
        {
        }

        private RowWriter(BinaryWriter writer, MarshalSerializerOptions options)
        {
            _writer = writer;
            _options = options;
            _legacy = options.Version == MarshalSerializerVersion.Legacy;
        }

        // One cache per integer width type (1 = byte, 2 = u16, 3 = u32, 4 = u64).
        private readonly List<(ushort Index, ulong Value)>[] _caches =
            [new List<(ushort, ulong)>(), new List<(ushort, ulong)>(), new List<(ushort, ulong)>(), new List<(ushort, ulong)>()];

        public void WriteEntry(string field, MarshalObject marshalObject)
        {
            if (!_options.FieldMappings.TryGetIndex(field, out var index))
                throw new Exception($"Field (name: {field}) not found");

            WriteEntry(index.Value, marshalObject);
        }

        public void WriteEntry(ushort fieldIndex, MarshalObject marshalObject)
        {
            switch (marshalObject.Type)
            {
                case FieldType.Byte:
                    AddInteger(fieldIndex, GetIntegerValue(marshalObject), 1);
                    break;
                case FieldType.Short:
                    {
                        var shortValue = GetIntegerValue(marshalObject);
                        AddInteger(fieldIndex, shortValue, shortValue <= 0xFF ? 1 : 2);
                        break;
                    }
                case FieldType.Int:
                case FieldType.Float:
                    var intValue = GetIntegerValue(marshalObject);
                    AddInteger(fieldIndex, intValue,
                        intValue <= 0xFF ? 1 : intValue <= 0xFFFF ? 2 : 3);
                    break;
                case FieldType.Long:
                case FieldType.Double:
                case FieldType.DateTime:
                    AddInteger(fieldIndex, GetIntegerValue(marshalObject), 4);
                    break;
                case FieldType.String:
                    WriteString(fieldIndex, marshalObject);
                    break;
                case FieldType.DataSet:
                    WriteDataSet(fieldIndex, marshalObject);
                    break;
                case FieldType.Guid:
                    WriteGuid(fieldIndex, marshalObject);
                    break;
                case FieldType.Blob:
                    WriteBlob(fieldIndex, marshalObject);
                    break;
                default:
                    throw new Exception($"Unsupported field type: {marshalObject.Type}");
            }
        }

        /// <summary>Flushes all remaining integer caches in width order (1, 2, 3, 4).</summary>
        public void Flush()
        {
            for (var type = 1; type <= 4; type++)
            {
                FlushCache(type);
            }
        }

        private static ulong GetIntegerValue(MarshalObject marshalObject)
        {
            if (marshalObject.Value is DateTime dateTime)
                return (ulong)new DateTimeOffset(dateTime.ToUniversalTime()).ToUnixTimeSeconds();
            if (marshalObject.Value is float f)
                return BitConverter.SingleToUInt32Bits(f);
            if (marshalObject.Value is double d)
                return BitConverter.DoubleToUInt64Bits(d);

            return Convert.ToUInt64(marshalObject.Value);
        }

        private void AddInteger(ushort fieldIndex, ulong value, int type)
        {
            var cache = _caches[type - 1];
            cache.Add((fieldIndex, value));

            if (cache.Count == 32)
            {
                FlushCache(type);
            }
        }

        private void FlushCache(int type)
        {
            var cache = _caches[type - 1];

            if (cache.Count == 0)
                return;

            var count = cache.Count;

            _writer.Write((ushort)((type << 12) | count));

            foreach (var (index, _) in cache)
            {
                _writer.Write(index);
            }

            foreach (var (_, value) in cache)
            {
                switch (type)
                {
                    case 1:
                        _writer.Write((byte)value);
                        break;
                    case 2:
                        _writer.Write((ushort)value);
                        break;
                    case 3:
                        _writer.Write((uint)value);
                        break;
                    case 4:
                        _writer.Write(value);
                        break;
                }
            }

            cache.Clear();
        }

        private void WriteString(ushort fieldIndex, MarshalObject marshalObject)
        {
            if (_legacy)
            {
                WriteLegacyString(fieldIndex, marshalObject);
                return;
            }

            var value = (string)marshalObject.Value;

            byte[] bytes;
            ushort length;
            ushort encoding;

            switch (marshalObject.Flags & (MarshalFlags.Utf16 | MarshalFlags.Utf32 | MarshalFlags.Utf8 | MarshalFlags.Ascii))
            {
                case MarshalFlags.Utf32:
                    encoding = 3;
                    length = (ushort)value.EnumerateRunes().Count();
                    bytes = Encoding.UTF32.GetBytes(value);
                    break;
                case MarshalFlags.Utf8:
                    encoding = 1;
                    length = (ushort)Encoding.UTF8.GetByteCount(value);
                    bytes = Encoding.UTF8.GetBytes(value);
                    break;
                case MarshalFlags.Ascii:
                    encoding = 5;
                    length = (ushort)value.Length;
                    bytes = Encoding.Latin1.GetBytes(value);
                    break;
                default:
                    encoding = 4;
                    length = (ushort)value.Length;
                    bytes = Encoding.Unicode.GetBytes(value);
                    break;
            }

            _writer.Write((ushort)((10 << 12) | encoding));
            _writer.Write(fieldIndex);
            _writer.Write(length);
            _writer.Write(bytes);
        }

        /// <summary>
        /// Writes a string the way the legacy writer did: strings of at most three
        /// UTF-16 units are stored inline (header type 9); longer strings use type 5
        /// with the length high bit selecting UTF-16, otherwise one byte per character.
        /// </summary>
        private void WriteLegacyString(ushort fieldIndex, MarshalObject marshalObject)
        {
            var value = (string)marshalObject.Value;
            var units = value.Length;

            if (units <= 3)
            {
                _writer.Write((ushort)((9 << 12) | units));
                _writer.Write(fieldIndex);
                _writer.Write(Encoding.Unicode.GetBytes(value));
                return;
            }

            var utf16 = marshalObject.Flags is MarshalFlags.Utf16 or MarshalFlags.Utf8 or MarshalFlags.Utf32;

            _writer.Write((ushort)((5 << 12) | 1));
            _writer.Write(fieldIndex);

            if (utf16)
            {
                if (units > 0x7FFF)
                    throw new Exception($"String too long for the legacy format: {units} UTF-16 units.");

                _writer.Write((ushort)(0x8000 | units));
                _writer.Write(Encoding.Unicode.GetBytes(value));
            }
            else
            {
                var bytes = Encoding.Latin1.GetBytes(value);
                _writer.Write((ushort)bytes.Length);
                _writer.Write(bytes);
            }
        }

        private void WriteDataSet(ushort fieldIndex, MarshalObject marshalObject)
        {
            var value = (IList<Dictionary<string, MarshalObject>>)marshalObject.Value;

            _writer.Write((ushort)((6 << 12) | 1));
            _writer.Write(fieldIndex);

            if (value.Count > ushort.MaxValue)
            {
                _writer.Write(ushort.MaxValue);
                _writer.Write((uint)value.Count);
            }
            else
            {
                _writer.Write((ushort)value.Count);
            }

            foreach (var row in value)
            {
                _writer.Write((ushort)row.Count);

                // Each row is written by its own CMarshalRow::Store call in the game,
                // which uses fresh integer caches flushed at the end of the row.
                var rowWriter = new RowWriter(_writer, _options);

                foreach (var entry in row)
                {
                    rowWriter.WriteEntry(entry.Key, entry.Value);
                }

                rowWriter.Flush();
            }
        }

        private void WriteGuid(ushort fieldIndex, MarshalObject marshalObject)
        {
            _writer.Write((ushort)((7 << 12) | 1));
            _writer.Write(fieldIndex);
            _writer.Write(((Guid)marshalObject.Value).ToByteArray());
        }

        private void WriteBlob(ushort fieldIndex, MarshalObject marshalObject)
        {
            var value = (byte[])marshalObject.Value;

            _writer.Write((ushort)((8 << 12) | 1));
            _writer.Write(fieldIndex);
            _writer.Write((ushort)value.Length);
            _writer.Write(value);
        }
    }
}
