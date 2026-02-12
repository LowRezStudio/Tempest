using System.Text.Json.Serialization;

namespace MarshalLib;

[JsonConverter(typeof(JsonStringEnumConverter<MarshalSerializerVersion>))]
public enum MarshalSerializerVersion
{
    Modern,
    Legacy
}
