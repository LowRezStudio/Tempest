using System.Text.Json.Serialization;

namespace MarshalLib;

[JsonConverter(typeof(JsonStringEnumConverter<FieldType>))]
public enum FieldType : ushort
{
    Byte = 2,
    Unsigned = 3,
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
    Blob = 15,
    AccountId = 0x0209,
    CharacterId = 0x0309,
    ClanId = 0x0409,
    InstanceId = 0x0709,
    MatchId = 0x0809,
    PlayerId = 0x0909,
    QueueId = 0x0A09,
    ServerId = 0x0B09,
    TeamId = 0x0C09,
    Custom = 0xFFFE,
    Grouped = 0xFFFF
}
