using System.Text.Json.Serialization;

namespace MarshalLib;

[JsonConverter(typeof(JsonStringEnumConverter<FieldType>))]
public enum FieldType : ushort
{
    Byte = 2,
    Short = 4,
    Int = 5,
    Float = 6,
    Double = 7,
    Long = 8,
    Id = 9,
    DateTime = 10,
    String = 12,
    DataSet = 13,
    Guid = 14,
    Blob = 15
}
