using System.Text.Json.Serialization;

namespace MarshalLib;

[JsonSerializable(typeof(MarshalFunction))]
[JsonSerializable(typeof(MarshalObject))]
[JsonSerializable(typeof(FieldType))]
[JsonSerializable(typeof(MarshalFlags))]
[JsonSerializable(typeof(MarshalSerializerVersion))]
[JsonSerializable(typeof(Dictionary<string, MarshalObject>))]
[JsonSerializable(typeof(List<Dictionary<string, MarshalObject>>))]
[JsonSerializable(typeof(JsonNumberEnumConverter<MarshalFlags>))]
[JsonSerializable(typeof(JsonNumberEnumConverter<FieldType>))]
[JsonSerializable(typeof(JsonNumberEnumConverter<MarshalSerializerVersion>))]
[JsonSerializable(typeof(IList<Dictionary<string, MarshalObject>>))]
[JsonSerializable(typeof(Guid))]
[JsonSerializable(typeof(DateTime))]
[JsonSerializable(typeof(byte))]
[JsonSerializable(typeof(ushort))]
[JsonSerializable(typeof(uint))]
[JsonSerializable(typeof(ulong))]
[JsonSerializable(typeof(string))]
[JsonSerializable(typeof(byte[]))]
public partial class MarshalSourceGenerationContext : JsonSerializerContext
{
}
