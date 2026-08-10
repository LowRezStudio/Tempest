using System.Globalization;
using System.Text;
using System.Text.Json;
using Microsoft.Data.Sqlite;

namespace MarshalLib.Database;

/// <summary>
/// Shared schema and value conversion rules for the SQLite marshal export format.
/// Both the exporter and the importer use this single source of truth, so the
/// on-disk schema is defined in exactly one place.
/// </summary>
/// <remarks>
/// Column declared types encode both the marshal field type and the string
/// encoding flags, e.g. <c>BYTE</c>, <c>LONG</c>, <c>TEXT_UTF16</c>. This keeps
/// the schema self-describing without a separate type registry table.
/// SQLite affinity still behaves sensibly: <c>INT</c> keeps INTEGER affinity,
/// <c>FLOAT</c>/<c>DOUBLE</c> get REAL affinity, <c>TEXT_*</c> get TEXT affinity,
/// and the remaining names fall back to NUMERIC affinity (integers stay INTEGER).
/// </remarks>
internal static class MarshalSqliteSchema
{
    public const string FunctionMetadataTable = "FUNCTION_METADATA";

    /// <summary>
    /// Records the ordinal of data-set fields that have no rows. A data set table
    /// without rows stores no per-row ordinals, so its position in the parent row
    /// (or the function row) would otherwise be lost on a round trip.
    /// </summary>
    public const string DataSetFieldsTable = "DATASET_FIELDS";

    public const string RowIdColumn = "row_id";
    public const string FunctionIdColumn = "marshal_function_id";
    public const string ParentRowIdColumn = "parent_row_id";
    public const string ParentTableColumn = "parent_table";
    public const string FieldOrdinalColumn = "field_ordinal";

    /// <summary>
    /// Optional per-row record of the data column sequence (comma-separated
    /// normalized column names). Written only when a row's entry order deviates
    /// from the table's first-seen column order, so heterogeneous rows survive
    /// the round trip without duplicating the order for every row.
    /// </summary>
    public const string EntryOrderColumn = "entry_order";

    public const string TableNameColumn = "table_name";

    public const string VersionColumn = "marshal_version";
    public const string FunctionColumn = "marshal_function";
    public const string FunctionNameColumn = "marshal_function_name";

    /// <summary>Columns present on every data set table.</summary>
    public static readonly string[] DataSetTableColumns =
        [RowIdColumn, FunctionIdColumn, ParentRowIdColumn, ParentTableColumn, FieldOrdinalColumn, EntryOrderColumn];

    /// <summary>Columns present on the function metadata table.</summary>
    public static readonly string[] FunctionMetadataColumns =
        [FunctionIdColumn, VersionColumn, FunctionColumn, FunctionNameColumn];

    /// <summary>Declared type names (also used as the marshal type registry).</summary>
    public static string GetDeclaredType(MarshalObject marshalObject)
    {
        return marshalObject.Type switch
        {
            FieldType.Byte => "BYTE",
            FieldType.Short => "SHORT",
            FieldType.Int => "INT",
            FieldType.Long => "LONG",
            FieldType.Float => "FLOAT",
            FieldType.Double => "DOUBLE",
            FieldType.DateTime => "DATETIME",
            FieldType.Guid => "GUID",
            FieldType.Blob => "BLOB",
            FieldType.String => marshalObject.Flags switch
            {
                MarshalFlags.Utf16 => "TEXT_UTF16",
                MarshalFlags.Utf32 => "TEXT_UTF32",
                MarshalFlags.Utf8 => "TEXT_UTF8",
                MarshalFlags.Ascii => "TEXT_ASCII",
                _ => "TEXT"
            },
            _ => "TEXT"
        };
    }

    /// <summary>
    /// Declares a column from the field's declared type in the game's token
    /// table when available, so edited values re-encode with the same width
    /// rules the game uses. The declared type is only trusted when its kind
    /// matches the observed value's kind (the wire is the truth for round trips);
    /// otherwise the observed type is used. String encoding flags always come
    /// from the observed value.
    /// </summary>
    public static string GetDeclaredType(MarshalObject marshalObject, string fieldName, FieldMappings? fieldMappings)
    {
        if (fieldMappings != null
            && fieldMappings.TryGetIndex(fieldName, out var index)
            && fieldMappings.Get(index.Value) is { } descriptor)
        {
            var wireType = (ushort)descriptor.Type & 0xFF;
            var observed = marshalObject.Type;

            // The declared type is only trusted when its width semantics match
            // the observed value: fixed-width kinds (Byte, 64-bit) must match
            // exactly, value-driven kinds (Short, Int/Float) accept narrower
            // observed values.
            switch (wireType)
            {
                case 2 when observed == FieldType.Byte: // Byte - always 8-bit
                    return "BYTE";
                case 4 when observed is FieldType.Byte or FieldType.Short: // Short - value-driven
                    return "SHORT";
                case 3 or 5 or 6 when observed is FieldType.Byte or FieldType.Short or FieldType.Int or FieldType.Float:
                    // Unsigned, Int, Float - width is value-driven
                    return "INT";
                case 7 or 8 or 9 or 10 when observed is FieldType.Long or FieldType.Double or FieldType.DateTime:
                    // Double, Long, Id, DateTime - always 64-bit
                    return "LONG";
                case 12 when observed == FieldType.String: // String
                    return GetDeclaredType(marshalObject);
                case 14 when observed == FieldType.Guid: // Guid
                    return "GUID";
                case 15 when observed == FieldType.Blob: // Blob
                    return "BLOB";
            }
        }

        return GetDeclaredType(marshalObject);
    }

    public static bool TryGetMarshalType(string declaredType, out FieldType type, out MarshalFlags flags)
    {
        type = FieldType.String;
        flags = MarshalFlags.None;

        switch (declaredType)
        {
            case "BYTE":
                type = FieldType.Byte;
                return true;
            case "SHORT":
                type = FieldType.Short;
                return true;
            case "INT":
                type = FieldType.Int;
                return true;
            case "LONG":
                type = FieldType.Long;
                return true;
            case "FLOAT":
                type = FieldType.Float;
                return true;
            case "DOUBLE":
                type = FieldType.Double;
                return true;
            case "DATETIME":
                type = FieldType.DateTime;
                return true;
            case "GUID":
                type = FieldType.Guid;
                return true;
            case "BLOB":
                type = FieldType.Blob;
                return true;
            case "TEXT":
                type = FieldType.String;
                return true;
            case "TEXT_UTF16":
                type = FieldType.String;
                flags = MarshalFlags.Utf16;
                return true;
            case "TEXT_UTF32":
                type = FieldType.String;
                flags = MarshalFlags.Utf32;
                return true;
            case "TEXT_UTF8":
                type = FieldType.String;
                flags = MarshalFlags.Utf8;
                return true;
            case "TEXT_ASCII":
                type = FieldType.String;
                flags = MarshalFlags.Ascii;
                return true;
            default:
                return false;
        }
    }

    /// <summary>Converts a marshal value to its SQLite storage representation.</summary>
    public static object? ToValue(MarshalObject marshalObject)
    {
        switch (marshalObject.Type)
        {
            case FieldType.Byte:
                // Fast path: the value is the boxed primitive the deserializer produced.
                return marshalObject.Value is byte byteValue
                    ? (long)byteValue
                    : Convert.ToInt64(marshalObject.Value, CultureInfo.InvariantCulture);
            case FieldType.Short:
                return marshalObject.Value is ushort shortValue
                    ? (long)shortValue
                    : Convert.ToInt64(marshalObject.Value, CultureInfo.InvariantCulture);
            case FieldType.Int:
                return marshalObject.Value is uint intValue
                    ? (long)intValue
                    : Convert.ToInt64(marshalObject.Value, CultureInfo.InvariantCulture);
            case FieldType.Long:
                // Bit-preserving: u64 values above long.MaxValue round-trip as negative.
                return marshalObject.Value is ulong longValue
                    ? unchecked((long)longValue)
                    : unchecked((long)Convert.ToUInt64(marshalObject.Value, CultureInfo.InvariantCulture));
            case FieldType.Float:
                // Stored as a real number so it is editable; bit patterns are
                // converted back on import.
                return marshalObject.Value is float f
                    ? f
                    : BitConverter.UInt32BitsToSingle(Convert.ToUInt32(marshalObject.Value, CultureInfo.InvariantCulture));
            case FieldType.Double:
                return marshalObject.Value is double d
                    ? d
                    : BitConverter.UInt64BitsToDouble(Convert.ToUInt64(marshalObject.Value, CultureInfo.InvariantCulture));
            case FieldType.DateTime:
                return marshalObject.Value is DateTime dateTime
                    ? dateTime.ToString("O", CultureInfo.InvariantCulture)
                    : marshalObject.Value?.ToString();
            case FieldType.Guid:
                return marshalObject.Value is Guid guid ? guid.ToString("D") : marshalObject.Value?.ToString();
            case FieldType.Blob:
                return marshalObject.Value as byte[];
            case FieldType.String:
                return marshalObject.Value?.ToString();
            default:
                return JsonSerializer.Serialize(marshalObject, MarshalSourceGenerationContext.Default.MarshalObject);
        }
    }

    /// <summary>
    /// Converts a SQLite value back into a marshal value. Integer widths use
    /// unchecked (bit-preserving) conversions so values round-trip exactly.
    /// For legacy databases, ASCII-able strings stored in UTF-16 columns are
    /// downgraded to the single-byte flag so the legacy writer reproduces the
    /// original wire bytes.
    /// </summary>
    public static MarshalObject FromValue(FieldType type, MarshalFlags flags, object? value, bool downgradeAsciiStrings = false)
    {
        switch (type)
        {
            case FieldType.Byte:
                return new MarshalObject(unchecked((byte)ToInt64(value)));
            case FieldType.Short:
                return new MarshalObject(unchecked((ushort)ToInt64(value)));
            case FieldType.Int:
                return new MarshalObject(unchecked((uint)ToInt64(value)));
            case FieldType.Long:
                return new MarshalObject(unchecked((ulong)ToInt64(value)));
            case FieldType.Float:
                return new MarshalObject(BitConverter.SingleToUInt32Bits((float)ToDouble(value)));
            case FieldType.Double:
                return new MarshalObject(BitConverter.DoubleToUInt64Bits(ToDouble(value)));
            case FieldType.DateTime:
                return new MarshalObject(DateTime.Parse(Convert.ToString(value, CultureInfo.InvariantCulture) ?? string.Empty, CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind));
            case FieldType.Guid:
                return new MarshalObject(Guid.Parse(Convert.ToString(value, CultureInfo.InvariantCulture) ?? string.Empty));
            case FieldType.Blob:
                return new MarshalObject(value as byte[] ?? []);
            case FieldType.String:
                if (downgradeAsciiStrings && flags == MarshalFlags.Utf16
                    && Convert.ToString(value, CultureInfo.InvariantCulture) is { } text
                    && text.All(c => c <= 0xFF))
                {
                    flags = MarshalFlags.Ascii;
                }

                return new MarshalObject(Convert.ToString(value, CultureInfo.InvariantCulture) ?? string.Empty, flags);
            default:
                throw new InvalidOperationException($"Unsupported field type: {type}");
        }
    }

    private static long ToInt64(object? value)
    {
        return value switch
        {
            long l => l,
            double d => (long)d,
            decimal m => (long)m,
            string s => long.Parse(s, CultureInfo.InvariantCulture),
            _ => Convert.ToInt64(value, CultureInfo.InvariantCulture)
        };
    }

    private static double ToDouble(object? value)
    {
        return value switch
        {
            double d => d,
            long l => l,
            string s => double.Parse(s, CultureInfo.InvariantCulture),
            _ => Convert.ToDouble(value, CultureInfo.InvariantCulture)
        };
    }

    /// <summary>
    /// Normalizes a marshal field name into a SQLite-safe identifier. All real
    /// game field names are uppercase alphanumeric plus underscores, for which
    /// this is the identity function, so the original name is recoverable.
    /// </summary>
    public static string NormalizeIdentifier(string value)
    {
        if (string.IsNullOrWhiteSpace(value))
            return "UNKNOWN";

        var builder = new StringBuilder(value.Length);
        foreach (var ch in value)
        {
            builder.Append(char.IsLetterOrDigit(ch) ? char.ToUpperInvariant(ch) : '_');
        }

        var result = builder.ToString().Trim('_');
        return result.Length == 0 ? "UNKNOWN" : result;
    }

    public static string QuoteIdentifier(string identifier)
    {
        return "\"" + identifier.Replace("\"", "\"\"") + "\"";
    }

    public static string JoinIdentifiers(IEnumerable<string> identifiers)
    {
        return string.Join(", ", identifiers.Select(QuoteIdentifier));
    }
}
