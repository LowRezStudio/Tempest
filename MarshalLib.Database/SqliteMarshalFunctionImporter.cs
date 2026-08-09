using Microsoft.Data.Sqlite;

namespace MarshalLib.Database;

/// <summary>
/// Imports a <see cref="MarshalFunction"/> from a SQLite database produced by
/// <see cref="SqliteMarshalFunctionExporter"/>, so the SQLite format is a full
/// round trip: marshal → SQLite (edit) → marshal.
/// </summary>
/// <remarks>
/// Field names and entry order are recovered from the schema: data columns are
/// read back in table order (first-seen order) and data-set entries are placed
/// at their recorded <c>field_ordinal</c> within the parent row. Column declared
/// types (see <see cref="MarshalSqliteSchema"/>) recover the marshal field type
/// and string encoding flags. NULL values are treated as absent fields and are
/// skipped, so deleting a value in SQL removes the field from the marshal.
/// Tables and columns that are not part of the marshal schema are ignored.
/// </remarks>
public static class SqliteMarshalFunctionImporter
{
    public static MarshalFunction Import(string connectionString)
    {
        using var connection = new SqliteConnection(connectionString);
        connection.Open();
        return Import(connection);
    }

    public static MarshalFunction Import(SqliteConnection connection)
    {
        var metadata = ReadMetadata(connection);
        var dataSetTables = DiscoverDataSetTables(connection);

        // (parentTable, parentRowId) -> child data set entries, plus the top-level entries.
        var childrenByParent = new Dictionary<(string, long), List<DataSetEntry>>();
        var topLevel = new List<DataSetEntry>();

        foreach (var table in dataSetTables.Values)
        {
            foreach (var group in table.Rows.GroupBy(r => (r.ParentRowId, r.FieldOrdinal)))
            {
                var entry = new DataSetEntry(table.Name, group.Key.FieldOrdinal, group.ToList());

                if (group.Key.ParentRowId is long parentRowId)
                {
                    var parentTable = group.First().ParentTable
                        ?? throw new InvalidOperationException($"Data set row in \"{table.Name}\" has a parent row id but no parent table.");

                    if (!childrenByParent.TryGetValue((parentTable, parentRowId), out var list))
                    {
                        list = [];
                        childrenByParent[(parentTable, parentRowId)] = list;
                    }

                    list.Add(entry);
                }
                else
                {
                    topLevel.Add(entry);
                }
            }
        }

        // Data sets without rows have no per-row ordinals; attach them at their
        // recorded ordinals to the recorded parent row (or the function).
        foreach (var field in ReadDataSetFields(connection))
        {
            if (field.ParentTable is null)
            {
                if (!topLevel.Any(e => e.FieldName == field.TableName && e.Ordinal == field.FieldOrdinal))
                    topLevel.Add(new DataSetEntry(field.TableName, field.FieldOrdinal, []));
            }
            else if (field.ParentRowId is long parentRowId)
            {
                var key = (field.ParentTable, parentRowId);

                if (!childrenByParent.TryGetValue(key, out var list))
                {
                    list = [];
                    childrenByParent[key] = list;
                }

                if (!list.Any(e => e.FieldName == field.TableName && e.Ordinal == field.FieldOrdinal))
                    list.Add(new DataSetEntry(field.TableName, field.FieldOrdinal, []));
            }
        }

        var rows = MergeEntries(metadata.Columns, metadata.Values, topLevel, dataSetTables, childrenByParent);

        return new MarshalFunction
        {
            Version = metadata.Version,
            Function = metadata.Function,
            FunctionName = metadata.FunctionName,
            Rows = rows
        };
    }

    private static Dictionary<string, MarshalObject> MergeEntries(
        IReadOnlyList<Column> columns,
        IReadOnlyDictionary<string, object?> values,
        List<DataSetEntry> dataSets,
        Dictionary<string, DataSetTable> tables,
        Dictionary<(string, long), List<DataSetEntry>> childrenByParent)
    {
        var result = new Dictionary<string, MarshalObject>();

        // Columns with NULL values are absent fields and take no entry slot.
        var presentColumns = columns.Where(c => values.TryGetValue(c.Name, out var v) && v != null).ToList();
        var total = presentColumns.Count + dataSets.Count;
        var columnIndex = 0;

        for (var ordinal = 0; ordinal < total; ordinal++)
        {
            var dataSet = dataSets.FirstOrDefault(d => d.Ordinal == ordinal);

            if (dataSet != null)
            {
                result[dataSet.FieldName] = BuildDataSet(dataSet, tables, childrenByParent);
            }
            else
            {
                var column = presentColumns[columnIndex++];
                result[column.Name] = MarshalSqliteSchema.FromValue(column.Type, column.Flags, values[column.Name]);
            }
        }

        return result;
    }

    private static MarshalObject BuildDataSet(
        DataSetEntry entry,
        Dictionary<string, DataSetTable> tables,
        Dictionary<(string, long), List<DataSetEntry>> childrenByParent)
    {
        var rows = new List<Dictionary<string, MarshalObject>>(entry.Rows.Count);
        var table = tables.TryGetValue(entry.FieldName, out var found) ? found : new DataSetTable(entry.FieldName, [], []);

        foreach (var rawRow in entry.Rows)
        {
            var children = childrenByParent.GetValueOrDefault((entry.FieldName, rawRow.RowId)) ?? [];
            var columns = rawRow.EntryOrder is string entryOrder
                ? ReorderColumns(table.Columns, entryOrder)
                : table.Columns;
            rows.Add(MergeEntries(columns, rawRow.Values, children, tables, childrenByParent));
        }

        return new MarshalObject(rows);
    }

    /// <summary>Orders the table's columns by a recorded entry sequence.</summary>
    private static IReadOnlyList<Column> ReorderColumns(IReadOnlyList<Column> columns, string entryOrder)
    {
        var byName = columns.ToDictionary(c => c.Name, StringComparer.OrdinalIgnoreCase);
        var reordered = new List<Column>(columns.Count);

        foreach (var name in entryOrder.Split(','))
        {
            if (byName.TryGetValue(name, out var column))
                reordered.Add(column);
        }

        return reordered;
    }

    private static FunctionMetadata ReadMetadata(SqliteConnection connection)
    {
        using var command = connection.CreateCommand();
        command.CommandText = $"SELECT * FROM {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionMetadataTable)} LIMIT 1;";

        using var reader = command.ExecuteReader();
        if (!reader.Read())
            throw new InvalidOperationException($"No function metadata found in {MarshalSqliteSchema.FunctionMetadataTable}.");

        var columns = GetDataColumns(connection, MarshalSqliteSchema.FunctionMetadataTable);
        var values = new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase);

        foreach (var column in columns)
        {
            var raw = reader[column.Name];
            values[column.Name] = raw is DBNull ? null : raw;
        }

        var version = MarshalSerializerVersion.Modern;
        if (reader.GetValue(reader.GetOrdinal(MarshalSqliteSchema.VersionColumn)) is not DBNull)
            version = (MarshalSerializerVersion)reader.GetInt32(reader.GetOrdinal(MarshalSqliteSchema.VersionColumn));

        var function = 0u;
        if (reader.GetValue(reader.GetOrdinal(MarshalSqliteSchema.FunctionColumn)) is not DBNull)
            function = unchecked((uint)reader.GetInt64(reader.GetOrdinal(MarshalSqliteSchema.FunctionColumn)));

        string? functionName = null;
        if (reader.GetValue(reader.GetOrdinal(MarshalSqliteSchema.FunctionNameColumn)) is not DBNull)
            functionName = reader.GetString(reader.GetOrdinal(MarshalSqliteSchema.FunctionNameColumn));

        return new FunctionMetadata(version, function, functionName, columns, values);
    }

    private static List<DataSetField> ReadDataSetFields(SqliteConnection connection)
    {
        var fields = new List<DataSetField>();

        using (var existsCommand = connection.CreateCommand())
        {
            existsCommand.CommandText =
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = $name;";
            existsCommand.Parameters.AddWithValue("$name", MarshalSqliteSchema.DataSetFieldsTable);

            if (Convert.ToInt64(existsCommand.ExecuteScalar(), System.Globalization.CultureInfo.InvariantCulture) == 0)
                return fields;
        }

        using var command = connection.CreateCommand();
        command.CommandText = $"SELECT * FROM {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.DataSetFieldsTable)};";

        using var reader = command.ExecuteReader();
        while (reader.Read())
        {
            var tableName = reader.GetString(reader.GetOrdinal(MarshalSqliteSchema.TableNameColumn));
            var parentTable = reader.IsDBNull(reader.GetOrdinal(MarshalSqliteSchema.ParentTableColumn))
                ? null
                : reader.GetString(reader.GetOrdinal(MarshalSqliteSchema.ParentTableColumn));
            var parentRowId = reader.IsDBNull(reader.GetOrdinal(MarshalSqliteSchema.ParentRowIdColumn))
                ? (long?)null
                : reader.GetInt64(reader.GetOrdinal(MarshalSqliteSchema.ParentRowIdColumn));
            var fieldOrdinal = reader.GetInt32(reader.GetOrdinal(MarshalSqliteSchema.FieldOrdinalColumn));
            fields.Add(new DataSetField(tableName, parentTable, parentRowId, fieldOrdinal));
        }

        return fields;
    }

    /// <summary>Discovers the data-set tables and reads all of their rows.</summary>
    private static Dictionary<string, DataSetTable> DiscoverDataSetTables(SqliteConnection connection)
    {
        var result = new Dictionary<string, DataSetTable>(StringComparer.OrdinalIgnoreCase);

        using var tablesCommand = connection.CreateCommand();
        tablesCommand.CommandText =
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%';";

        using var tablesReader = tablesCommand.ExecuteReader();
        var tableNames = new List<string>();

        while (tablesReader.Read())
        {
            var tableName = tablesReader.GetString(0);
            if (!string.Equals(tableName, MarshalSqliteSchema.FunctionMetadataTable, StringComparison.OrdinalIgnoreCase)
                && !string.Equals(tableName, MarshalSqliteSchema.DataSetFieldsTable, StringComparison.OrdinalIgnoreCase))
            {
                tableNames.Add(tableName);
            }
        }

        foreach (var tableName in tableNames)
        {
            var columns = GetDataColumns(connection, tableName);
            if (columns.Count == 0)
                continue;

            var rows = ReadDataSetRows(connection, tableName, columns);
            result[tableName] = new DataSetTable(tableName, columns, rows);
        }

        return result;
    }

    /// <summary>
    /// Returns the marshal data columns of a table (all columns except the
    /// reserved ones) with their recovered marshal types. Columns with unknown
    /// declared types (e.g. added by the user) are ignored.
    /// </summary>
    private static List<Column> GetDataColumns(SqliteConnection connection, string tableName)
    {
        var reserved = string.Equals(tableName, MarshalSqliteSchema.FunctionMetadataTable, StringComparison.OrdinalIgnoreCase)
            ? MarshalSqliteSchema.FunctionMetadataColumns
            : MarshalSqliteSchema.DataSetTableColumns;

        var columns = new List<Column>();

        using var pragmaCommand = connection.CreateCommand();
        pragmaCommand.CommandText = $"PRAGMA table_info({MarshalSqliteSchema.QuoteIdentifier(tableName)});";

        using var reader = pragmaCommand.ExecuteReader();
        while (reader.Read())
        {
            var name = reader.GetString(1);
            if (reserved.Contains(name, StringComparer.OrdinalIgnoreCase))
                continue;

            var declaredType = reader.GetString(2);
            if (!MarshalSqliteSchema.TryGetMarshalType(declaredType, out var type, out var flags))
                continue;

            columns.Add(new Column(name, type, flags));
        }

        return columns;
    }

    private static List<DataSetRow> ReadDataSetRows(SqliteConnection connection, string tableName, IReadOnlyList<Column> columns)
    {
        var rows = new List<DataSetRow>();

        var selectColumns = new List<string>
        {
            MarshalSqliteSchema.RowIdColumn,
            MarshalSqliteSchema.ParentRowIdColumn,
            MarshalSqliteSchema.ParentTableColumn,
            MarshalSqliteSchema.FieldOrdinalColumn,
            MarshalSqliteSchema.EntryOrderColumn
        };
        selectColumns.AddRange(columns.Select(c => c.Name));

        using var command = connection.CreateCommand();
        command.CommandText = $@"
SELECT {MarshalSqliteSchema.JoinIdentifiers(selectColumns)}
FROM {MarshalSqliteSchema.QuoteIdentifier(tableName)}
ORDER BY {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.RowIdColumn)};
";

        using var reader = command.ExecuteReader();
        while (reader.Read())
        {
            var rowId = reader.GetInt64(0);
            var parentRowId = reader.IsDBNull(1) ? (long?)null : reader.GetInt64(1);
            var parentTable = reader.IsDBNull(2) ? null : reader.GetString(2);
            var fieldOrdinal = reader.GetInt32(3);
            var entryOrder = reader.IsDBNull(4) ? null : reader.GetString(4);
            var values = new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase);

            for (var i = 0; i < columns.Count; i++)
            {
                var raw = reader.GetValue(5 + i);
                values[columns[i].Name] = raw is DBNull ? null : raw;
            }

            rows.Add(new DataSetRow(rowId, parentRowId, parentTable, fieldOrdinal, entryOrder, values));
        }

        return rows;
    }

    private sealed record FunctionMetadata(
        MarshalSerializerVersion Version,
        uint Function,
        string? FunctionName,
        IReadOnlyList<Column> Columns,
        IReadOnlyDictionary<string, object?> Values);

    private sealed record DataSetTable(string Name, IReadOnlyList<Column> Columns, IReadOnlyList<DataSetRow> Rows);

    private sealed record DataSetRow(
        long RowId,
        long? ParentRowId,
        string? ParentTable,
        int FieldOrdinal,
        string? EntryOrder,
        IReadOnlyDictionary<string, object?> Values);

    private sealed record DataSetEntry(string FieldName, int Ordinal, List<DataSetRow> Rows);

    private sealed record DataSetField(string TableName, string? ParentTable, long? ParentRowId, int FieldOrdinal);

    private readonly record struct Column(string Name, FieldType Type, MarshalFlags Flags);
}
