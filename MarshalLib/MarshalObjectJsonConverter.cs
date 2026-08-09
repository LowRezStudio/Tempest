using System.Buffers;
using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace MarshalLib;

public sealed class MarshalObjectJsonConverter : JsonConverter<MarshalObject>
{
    public override MarshalObject Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType != JsonTokenType.StartObject)
            throw new JsonException("MarshalObject must be an object.");

        FieldType? type = null;
        var flags = MarshalFlags.None;
        object? value = null;
        var hasValue = false;

        while (reader.Read())
        {
            if (reader.TokenType == JsonTokenType.EndObject)
                break;

            if (reader.TokenType != JsonTokenType.PropertyName)
                throw new JsonException("Unexpected token in MarshalObject.");

            // Match the known property names against the raw UTF-8 span so the
            // common case allocates no string at all.
            if (IsToken(ref reader, "Type"u8))
            {
                reader.Read();
                type = ReadFieldType(ref reader);
            }
            else if (IsToken(ref reader, "Flags"u8))
            {
                reader.Read();
                flags = ReadFlags(ref reader);
            }
            else if (IsToken(ref reader, "Value"u8))
            {
                reader.Read();
                if (type is null)
                    throw new JsonException("MarshalObject Value appears before Type.");

                value = ReadValue(ref reader, type.Value, options);
                hasValue = true;
            }
            else
            {
                reader.Skip();
            }
        }

        if (type is null)
            throw new JsonException("MarshalObject is missing Type property.");
        if (!hasValue)
            throw new JsonException("MarshalObject is missing Value property.");

        return new MarshalObject(type.Value, value!, flags);
    }

    public override void Write(Utf8JsonWriter writer, MarshalObject value, JsonSerializerOptions options)
    {
        writer.WriteStartObject();
        writer.WritePropertyName("Type");
        JsonSerializer.Serialize(writer, value.Type, options.GetTypeInfo(typeof(FieldType)));
        if (value.Flags != MarshalFlags.None)
        {
            writer.WritePropertyName("Flags");
            JsonSerializer.Serialize(writer, value.Flags, options.GetTypeInfo(typeof(MarshalFlags)));
        }
        writer.WritePropertyName("Value");
        if (value.Value is null)
        {
            writer.WriteNullValue();
        }
        else
        {
            JsonSerializer.Serialize(writer, value.Value, options.GetTypeInfo(value.Value.GetType()));
        }
        writer.WriteEndObject();
    }

    private static FieldType ReadFieldType(ref Utf8JsonReader reader)
    {
        return reader.TokenType switch
        {
            JsonTokenType.String => ParseFieldType(ref reader),
            JsonTokenType.Number => (FieldType)reader.GetUInt16(),
            _ => throw new JsonException("MarshalObject Type must be a string or number.")
        };
    }

    private static MarshalFlags ReadFlags(ref Utf8JsonReader reader)
    {
        return reader.TokenType switch
        {
            JsonTokenType.String => ParseFlags(ref reader),
            JsonTokenType.Number => (MarshalFlags)reader.GetUInt16(),
            JsonTokenType.Null => MarshalFlags.None,
            _ => throw new JsonException("MarshalObject Flags must be a string or number.")
        };
    }

    /// <summary>
    /// Fast path for the canonical enum names (the only ones that occur in
    /// practice): match the raw UTF-8 span without allocating. Anything else
    /// (non-canonical case, comma-joined flags, ...) falls back to
    /// <see cref="Enum.Parse{TEnum}"/> which preserves the previous behavior.
    /// </summary>
    private static FieldType ParseFieldType(ref Utf8JsonReader reader)
    {
        if (!reader.HasValueSequence)
        {
            var span = reader.ValueSpan;
            switch (span.Length)
            {
                case 3:
                    if (span.SequenceEqual("Int"u8)) return FieldType.Int;
                    break;
                case 4:
                    if (span.SequenceEqual("Byte"u8)) return FieldType.Byte;
                    if (span.SequenceEqual("Long"u8)) return FieldType.Long;
                    if (span.SequenceEqual("Guid"u8)) return FieldType.Guid;
                    if (span.SequenceEqual("Blob"u8)) return FieldType.Blob;
                    break;
                case 5:
                    if (span.SequenceEqual("Short"u8)) return FieldType.Short;
                    if (span.SequenceEqual("Float"u8)) return FieldType.Float;
                    break;
                case 6:
                    if (span.SequenceEqual("Double"u8)) return FieldType.Double;
                    if (span.SequenceEqual("String"u8)) return FieldType.String;
                    break;
                case 7:
                    if (span.SequenceEqual("DataSet"u8)) return FieldType.DataSet;
                    break;
                case 8:
                    if (span.SequenceEqual("DateTime"u8)) return FieldType.DateTime;
                    break;
            }
        }
        return Enum.Parse<FieldType>(reader.GetString() ?? string.Empty, ignoreCase: true);
    }

    private static MarshalFlags ParseFlags(ref Utf8JsonReader reader)
    {
        if (!reader.HasValueSequence)
        {
            var span = reader.ValueSpan;
            switch (span.Length)
            {
                case 4:
                    if (span.SequenceEqual("None"u8)) return MarshalFlags.None;
                    break;
                case 5:
                    if (span.SequenceEqual("Utf16"u8)) return MarshalFlags.Utf16;
                    if (span.SequenceEqual("Utf32"u8)) return MarshalFlags.Utf32;
                    if (span.SequenceEqual("Utf8"u8)) return MarshalFlags.Utf8;
                    break;
                case 6:
                    if (span.SequenceEqual("Ascii"u8)) return MarshalFlags.Ascii;
                    break;
            }
        }
        return Enum.Parse<MarshalFlags>(reader.GetString() ?? string.Empty, ignoreCase: true);
    }

    /// <summary>True if the current token's value equals <paramref name="value"/>.</summary>
    private static bool IsToken(ref Utf8JsonReader reader, ReadOnlySpan<byte> value)
    {
        if (!reader.HasValueSequence)
            return reader.ValueSpan.SequenceEqual(value);

        // The value spans a buffer boundary; compare via a sequence reader.
        var sequence = reader.ValueSequence;
        if (sequence.Length != value.Length)
            return false;
        var sequenceReader = new SequenceReader<byte>(sequence);
        return sequenceReader.IsNext(value, advancePast: false);
    }

    private object ReadValue(ref Utf8JsonReader reader, FieldType type, JsonSerializerOptions options)
    {
        return type switch
        {
            FieldType.Byte => reader.GetByte(),
            FieldType.Short => reader.GetUInt16(),
            FieldType.Int => reader.GetUInt32(),
            FieldType.Long => reader.GetUInt64(),
            FieldType.Float => ReadFloat(ref reader),
            FieldType.Double => ReadDouble(ref reader),
            FieldType.Guid => ReadGuid(ref reader),
            FieldType.Blob => ReadBlob(ref reader),
            FieldType.String => reader.TokenType == JsonTokenType.Null ? string.Empty : reader.GetString() ?? string.Empty,
            FieldType.DataSet => ReadDataSet(ref reader, options),
            FieldType.DateTime => ReadDateTime(ref reader),
            _ => throw new JsonException($"Unsupported field type: {type}")
        };
    }

    /// <summary>
    /// Reads a float field value: an integer number is the raw IEEE-754 bit
    /// pattern (as written by deserialization), any other number is the actual
    /// float value and is converted to bits for the wire.
    /// </summary>
    private static object ReadFloat(ref Utf8JsonReader reader)
    {
        if (reader.TokenType != JsonTokenType.Number)
            throw new JsonException("Value must be a number.");

        return reader.TryGetUInt32(out var bits)
            ? bits
            : BitConverter.SingleToUInt32Bits(reader.GetSingle());
    }

    /// <summary>
    /// Reads a double field value: an integer number is the raw IEEE-754 bit
    /// pattern (as written by deserialization), any other number is the actual
    /// double value and is converted to bits for the wire.
    /// </summary>
    private static object ReadDouble(ref Utf8JsonReader reader)
    {
        if (reader.TokenType != JsonTokenType.Number)
            throw new JsonException("Value must be a number.");

        return reader.TryGetUInt64(out var bits)
            ? bits
            : BitConverter.DoubleToUInt64Bits(reader.GetDouble());
    }

    private static object ReadGuid(ref Utf8JsonReader reader)
    {
        if (reader.TokenType != JsonTokenType.String)
            throw new JsonException("Guid value must be a string.");

        return Guid.Parse(reader.GetString() ?? string.Empty);
    }

    private static object ReadDateTime(ref Utf8JsonReader reader)
    {
        if (reader.TokenType != JsonTokenType.String)
            throw new JsonException("DateTime value must be a string.");

        return DateTime.Parse(reader.GetString() ?? string.Empty, CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind);
    }

    private static object ReadBlob(ref Utf8JsonReader reader)
    {
        if (reader.TokenType == JsonTokenType.String)
            return Convert.FromBase64String(reader.GetString() ?? string.Empty);

        if (reader.TokenType == JsonTokenType.StartArray)
        {
            var bytes = new List<byte>();
            while (reader.Read() && reader.TokenType != JsonTokenType.EndArray)
            {
                bytes.Add(reader.GetByte());
            }

            return bytes.ToArray();
        }

        throw new JsonException("Blob value must be a base64 string or number array.");
    }

    private object ReadDataSet(ref Utf8JsonReader reader, JsonSerializerOptions options)
    {
        var rows = new List<Dictionary<string, MarshalObject>>();

        if (reader.TokenType == JsonTokenType.Null)
            return rows;

        if (reader.TokenType != JsonTokenType.StartArray)
            throw new JsonException("DataSet value must be an array.");

        while (reader.Read() && reader.TokenType != JsonTokenType.EndArray)
        {
            if (reader.TokenType != JsonTokenType.StartObject)
                throw new JsonException("DataSet rows must be objects.");

            var row = new Dictionary<string, MarshalObject>();

            while (reader.Read() && reader.TokenType != JsonTokenType.EndObject)
            {
                if (reader.TokenType != JsonTokenType.PropertyName)
                    throw new JsonException("Unexpected token in DataSet row.");

                var name = reader.GetString() ?? string.Empty;
                reader.Read();
                row[name] = Read(ref reader, typeof(MarshalObject), options);
            }

            rows.Add(row);
        }

        return rows;
    }
}
