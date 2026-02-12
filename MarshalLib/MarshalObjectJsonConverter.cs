using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace MarshalLib;

public sealed class MarshalObjectJsonConverter : JsonConverter<MarshalObject>
{
    public override MarshalObject Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        using var document = JsonDocument.ParseValue(ref reader);
        var root = document.RootElement;

        var type = ReadFieldType(root, "Type");
        var flags = ReadFlags(root, "Flags");

        if (!root.TryGetProperty("Value", out var valueElement))
            throw new JsonException("MarshalObject is missing Value property.");

        var value = ReadValue(type, valueElement, options);
        return new MarshalObject(type, value, flags);
    }

    public override void Write(Utf8JsonWriter writer, MarshalObject value, JsonSerializerOptions options)
    {
        writer.WriteStartObject();
        writer.WritePropertyName("Type");
        JsonSerializer.Serialize(writer, value.Type, options);
        writer.WritePropertyName("Flags");
        JsonSerializer.Serialize(writer, value.Flags, options);
        writer.WritePropertyName("Value");
        JsonSerializer.Serialize(writer, value.Value, value.Value?.GetType() ?? typeof(object), options);
        writer.WriteEndObject();
    }

    private static FieldType ReadFieldType(JsonElement root, string propertyName)
    {
        if (!root.TryGetProperty(propertyName, out var element))
            throw new JsonException("MarshalObject is missing Type property.");

        return element.ValueKind switch
        {
            JsonValueKind.String => Enum.Parse<FieldType>(element.GetString() ?? string.Empty, ignoreCase: true),
            JsonValueKind.Number => (FieldType)element.GetUInt16(),
            _ => throw new JsonException("MarshalObject Type must be a string or number.")
        };
    }

    private static MarshalFlags ReadFlags(JsonElement root, string propertyName)
    {
        if (!root.TryGetProperty(propertyName, out var element))
            return MarshalFlags.None;

        return element.ValueKind switch
        {
            JsonValueKind.String => Enum.Parse<MarshalFlags>(element.GetString() ?? string.Empty, ignoreCase: true),
            JsonValueKind.Number => (MarshalFlags)element.GetUInt16(),
            JsonValueKind.Null => MarshalFlags.None,
            _ => throw new JsonException("MarshalObject Flags must be a string or number.")
        };
    }

    private static object ReadValue(FieldType type, JsonElement element, JsonSerializerOptions options)
    {
        return type switch
        {
            FieldType.Byte => Convert.ToByte(ReadNumber(element)),
            FieldType.Short => Convert.ToUInt16(ReadNumber(element)),
            FieldType.Int => Convert.ToUInt32(ReadNumber(element)),
            FieldType.Long => Convert.ToUInt64(ReadNumber(element)),
            FieldType.Guid => ReadGuid(element),
            FieldType.Blob => ReadBlob(element),
            FieldType.String => element.ValueKind == JsonValueKind.Null ? string.Empty : element.GetString() ?? string.Empty,
            FieldType.DataSet => ReadDataSet(element, options),
            FieldType.DateTime => ReadDateTime(element),
            _ => throw new JsonException($"Unsupported field type: {type}")
        };
    }

    private static ulong ReadNumber(JsonElement element)
    {
        if (element.ValueKind != JsonValueKind.Number)
            throw new JsonException("Value must be a number.");

        return element.GetUInt64();
    }

    private static object ReadGuid(JsonElement element)
    {
        if (element.ValueKind != JsonValueKind.String)
            throw new JsonException("Guid value must be a string.");

        return Guid.Parse(element.GetString() ?? string.Empty);
    }

    private static object ReadDateTime(JsonElement element)
    {
        if (element.ValueKind != JsonValueKind.String)
            throw new JsonException("DateTime value must be a string.");

        return DateTime.Parse(element.GetString() ?? string.Empty, CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind);
    }

    private static object ReadBlob(JsonElement element)
    {
        if (element.ValueKind == JsonValueKind.String)
            return Convert.FromBase64String(element.GetString() ?? string.Empty);

        if (element.ValueKind == JsonValueKind.Array)
        {
            var bytes = new List<byte>();
            foreach (var item in element.EnumerateArray())
            {
                bytes.Add(Convert.ToByte(ReadNumber(item)));
            }

            return bytes.ToArray();
        }

        throw new JsonException("Blob value must be a base64 string or number array.");
    }

    private static object ReadDataSet(JsonElement element, JsonSerializerOptions options)
    {
        if (element.ValueKind == JsonValueKind.Null)
            return new List<Dictionary<string, MarshalObject>>();

        return element.Deserialize<IList<Dictionary<string, MarshalObject>>>(options)
            ?? new List<Dictionary<string, MarshalObject>>();
    }
}
