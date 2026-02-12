using System.Text.Json.Serialization;

namespace MarshalLib;

[Flags]
[JsonConverter(typeof(JsonStringEnumConverter<MarshalFlags>))]
public enum MarshalFlags
{
    None = 0,
    Utf16 = 1 << 0,
    Utf32 = 1 << 1,
}
