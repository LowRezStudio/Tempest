using System.Globalization;
using System.Text;
using System.Text.Json;
using Microsoft.Data.Sqlite;
using MarshalLib;

namespace MarshalLib.Database;

public static class SqliteMarshalFunctionExporter
{
    private const string FunctionMetadataTable = "FUNCTION_METADATA";

    public static void Export(string connectionString, MarshalFunction function)
    {
        using var connection = new SqliteConnection(connectionString);
        connection.Open();
        Export(connection, function);
    }

    public static void Export(SqliteConnection connection, MarshalFunction function)
    {
        using var transaction = connection.BeginTransaction();

        EnsurePragmas(connection, transaction);
        EnsureFunctionMetadataTable(connection, transaction);

        var functionId = UpsertFunctionMetadata(connection, transaction, function);

        foreach (var (fieldName, marshalObject) in function.Rows)
        {
            if (marshalObject.Type == FieldType.DataSet)
            {
                ExportDataSet(connection, transaction, functionId, null, null, fieldName, marshalObject);
            }
        }

        transaction.Commit();
    }

    private static void EnsurePragmas(SqliteConnection connection, SqliteTransaction transaction)
    {
        using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = "PRAGMA foreign_keys = ON;";
        command.ExecuteNonQuery();
    }

    private static void EnsureFunctionMetadataTable(SqliteConnection connection, SqliteTransaction transaction)
    {
        using var createCommand = connection.CreateCommand();
        createCommand.Transaction = transaction;
        createCommand.CommandText = $@"
CREATE TABLE IF NOT EXISTS {QuoteIdentifier(FunctionMetadataTable)} (
    function_id INTEGER PRIMARY KEY AUTOINCREMENT,
    version INTEGER NOT NULL,
    function INTEGER NOT NULL,
    function_name TEXT NULL
);
";
        createCommand.ExecuteNonQuery();
    }

    private static long UpsertFunctionMetadata(
        SqliteConnection connection,
        SqliteTransaction transaction,
        MarshalFunction function)
    {
        var metadataColumns = new Dictionary<string, ColumnDefinition>(StringComparer.OrdinalIgnoreCase);

        foreach (var (fieldName, marshalObject) in function.Rows)
        {
            if (marshalObject.Type == FieldType.DataSet)
                continue;

            var columnName = NormalizeIdentifier(fieldName);
            metadataColumns[columnName] = new ColumnDefinition(columnName, GetSqliteType(marshalObject));
        }

        EnsureColumns(connection, transaction, FunctionMetadataTable, metadataColumns.Values);

        var insertColumns = new List<string> { "function_id", "version", "function", "function_name" };
        var insertValues = new List<string> { "$function_id", "$version", "$function", "$function_name" };

        foreach (var column in metadataColumns.Values)
        {
            insertColumns.Add(column.Name);
            insertValues.Add("$" + column.Name);
        }

        using var insertCommand = connection.CreateCommand();
        insertCommand.Transaction = transaction;
        insertCommand.CommandText = $@"
INSERT INTO {QuoteIdentifier(FunctionMetadataTable)} ({JoinIdentifiers(insertColumns)})
VALUES ({string.Join(", ", insertValues)});
SELECT last_insert_rowid();
";

        insertCommand.Parameters.AddWithValue("$function_id", DBNull.Value);
        insertCommand.Parameters.AddWithValue("$version", (int)function.Version);
        insertCommand.Parameters.AddWithValue("$function", unchecked((long)function.Function));
        insertCommand.Parameters.AddWithValue("$function_name", (object?)function.FunctionName ?? DBNull.Value);

        foreach (var (fieldName, marshalObject) in function.Rows)
        {
            if (marshalObject.Type == FieldType.DataSet)
                continue;

            var columnName = NormalizeIdentifier(fieldName);
            var value = ToSqliteValue(marshalObject);
            insertCommand.Parameters.AddWithValue("$" + columnName, value ?? DBNull.Value);
        }

        var result = insertCommand.ExecuteScalar();
        return result is long id ? id : Convert.ToInt64(result, CultureInfo.InvariantCulture);
    }

    private static void ExportDataSet(
        SqliteConnection connection,
        SqliteTransaction transaction,
        long functionId,
        long? parentRowId,
        string? parentTable,
        string fieldName,
        MarshalObject marshalObject)
    {
        var tableName = NormalizeIdentifier(fieldName);
        var rows = (IList<Dictionary<string, MarshalObject>>)marshalObject.Value;

        EnsureDataSetTable(connection, transaction, tableName);

        foreach (var row in rows)
        {
            var columnDefinitions = new List<ColumnDefinition>();
            var values = new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase);
            var nestedDataSets = new List<(string FieldName, MarshalObject Value)>();

            foreach (var (columnNameRaw, value) in row)
            {
                if (value.Type == FieldType.DataSet)
                {
                    nestedDataSets.Add((columnNameRaw, value));
                    continue;
                }

                var columnName = NormalizeIdentifier(columnNameRaw);
                var columnType = GetSqliteType(value);
                columnDefinitions.Add(new ColumnDefinition(columnName, columnType));
                values[columnName] = ToSqliteValue(value);
            }

            EnsureColumns(connection, transaction, tableName, columnDefinitions);

            var rowId = InsertDataSetRow(
                connection,
                transaction,
                tableName,
                functionId,
                parentRowId,
                parentTable,
                values);

            foreach (var (nestedFieldName, nestedValue) in nestedDataSets)
            {
                ExportDataSet(connection, transaction, functionId, rowId, tableName, nestedFieldName, nestedValue);
            }
        }
    }

    private static void EnsureDataSetTable(SqliteConnection connection, SqliteTransaction transaction, string tableName)
    {
        using var createCommand = connection.CreateCommand();
        createCommand.Transaction = transaction;
        createCommand.CommandText = $@"
CREATE TABLE IF NOT EXISTS {QuoteIdentifier(tableName)} (
    row_id INTEGER PRIMARY KEY AUTOINCREMENT,
    function_id INTEGER NOT NULL,
    parent_row_id INTEGER NULL,
    parent_table TEXT NULL,
    FOREIGN KEY (function_id) REFERENCES {QuoteIdentifier(FunctionMetadataTable)}(function_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS {QuoteIdentifier("ix_" + tableName + "_function_id")}
    ON {QuoteIdentifier(tableName)}(function_id);

CREATE INDEX IF NOT EXISTS {QuoteIdentifier("ix_" + tableName + "_parent_row_id")}
    ON {QuoteIdentifier(tableName)}(parent_row_id);
";
        createCommand.ExecuteNonQuery();
    }

    private static long InsertDataSetRow(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string tableName,
        long functionId,
        long? parentRowId,
        string? parentTable,
        IReadOnlyDictionary<string, object?> values)
    {
        var insertColumns = new List<string> { "function_id", "parent_row_id", "parent_table" };
        var insertValues = new List<string> { "$function_id", "$parent_row_id", "$parent_table" };

        foreach (var column in values.Keys)
        {
            insertColumns.Add(column);
            insertValues.Add("$" + column);
        }

        using var insertCommand = connection.CreateCommand();
        insertCommand.Transaction = transaction;
        insertCommand.CommandText = $@"
INSERT INTO {QuoteIdentifier(tableName)} ({JoinIdentifiers(insertColumns)})
VALUES ({string.Join(", ", insertValues)});
SELECT last_insert_rowid();
";

        insertCommand.Parameters.AddWithValue("$function_id", functionId);
        insertCommand.Parameters.AddWithValue("$parent_row_id", parentRowId.HasValue ? parentRowId.Value : DBNull.Value);
        insertCommand.Parameters.AddWithValue("$parent_table", (object?)parentTable ?? DBNull.Value);

        foreach (var (column, value) in values)
        {
            insertCommand.Parameters.AddWithValue("$" + column, value ?? DBNull.Value);
        }

        var result = insertCommand.ExecuteScalar();
        return result is long id ? id : Convert.ToInt64(result, CultureInfo.InvariantCulture);
    }

    private static void EnsureColumns(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string tableName,
        IEnumerable<ColumnDefinition> columns)
    {
        var existingColumns = GetExistingColumns(connection, transaction, tableName);

        foreach (var column in columns)
        {
            if (existingColumns.Contains(column.Name))
                continue;

            using var alterCommand = connection.CreateCommand();
            alterCommand.Transaction = transaction;
            alterCommand.CommandText = $@"
ALTER TABLE {QuoteIdentifier(tableName)}
ADD COLUMN {QuoteIdentifier(column.Name)} {column.Type};
";
            alterCommand.ExecuteNonQuery();
        }
    }

    private static HashSet<string> GetExistingColumns(SqliteConnection connection, SqliteTransaction transaction, string tableName)
    {
        using var pragmaCommand = connection.CreateCommand();
        pragmaCommand.Transaction = transaction;
        pragmaCommand.CommandText = $"PRAGMA table_info({QuoteIdentifier(tableName)});";

        using var reader = pragmaCommand.ExecuteReader();
        var columns = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        while (reader.Read())
        {
            var name = reader.GetString(1);
            columns.Add(name);
        }

        return columns;
    }

    private static string GetSqliteType(MarshalObject marshalObject)
    {
        return marshalObject.Type switch
        {
            FieldType.Byte => "INTEGER",
            FieldType.Short => "INTEGER",
            FieldType.Int => "INTEGER",
            FieldType.Long => "INTEGER",
            FieldType.Guid => "TEXT",
            FieldType.String => "TEXT",
            FieldType.Blob => "BLOB",
            FieldType.DateTime => "TEXT",
            _ => "TEXT"
        };
    }

    private static object? ToSqliteValue(MarshalObject marshalObject)
    {
        return marshalObject.Type switch
        {
            FieldType.Byte => Convert.ToInt64(marshalObject.Value, CultureInfo.InvariantCulture),
            FieldType.Short => Convert.ToInt64(marshalObject.Value, CultureInfo.InvariantCulture),
            FieldType.Int => Convert.ToInt64(marshalObject.Value, CultureInfo.InvariantCulture),
            FieldType.Long => Convert.ToInt64(marshalObject.Value, CultureInfo.InvariantCulture),
            FieldType.Guid => marshalObject.Value is Guid guid ? guid.ToString("D") : marshalObject.Value?.ToString(),
            FieldType.String => marshalObject.Value?.ToString(),
            FieldType.Blob => marshalObject.Value as byte[],
            FieldType.DateTime => marshalObject.Value is DateTime dateTime
                ? dateTime.ToString("O", CultureInfo.InvariantCulture)
                : marshalObject.Value?.ToString(),
            _ => SerializeFallback(marshalObject)
        };
    }

    private static string SerializeFallback(MarshalObject marshalObject)
    {
        return JsonSerializer.Serialize(marshalObject, MarshalSourceGenerationContext.Default.MarshalObject);
    }

    private static string NormalizeIdentifier(string value)
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

    private static string QuoteIdentifier(string identifier)
    {
        return "\"" + identifier.Replace("\"", "\"\"") + "\"";
    }

    private static string JoinIdentifiers(IEnumerable<string> identifiers)
    {
        return string.Join(", ", identifiers.Select(QuoteIdentifier));
    }

    private readonly record struct ColumnDefinition(string Name, string Type);
}
