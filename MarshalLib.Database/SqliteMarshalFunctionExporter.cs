using Microsoft.Data.Sqlite;

namespace MarshalLib.Database;

/// <summary>
/// Exports a <see cref="MarshalFunction"/> into a SQLite database.
/// </summary>
/// <remarks>
/// Schema (defined in <see cref="MarshalSqliteSchema"/>):
/// <list type="bullet">
/// <item><c>FUNCTION_METADATA</c> - one row per function; top-level non-data-set
/// fields are stored as typed columns.</item>
/// <item>One table per data-set field name - each row is one data-set row; the
/// <c>field_ordinal</c> column records the position of the data set entry within
/// its parent row so the original entry order survives a round trip.</item>
/// <item><c>DATASET_FIELDS</c> - records the parent position of data-set fields
/// that have no rows (empty data sets have no per-row ordinals).</item>
/// </list>
/// Column declared types encode the marshal field type and string encoding flags
/// (e.g. <c>BYTE</c>, <c>LONG</c>, <c>TEXT_UTF16</c>). A column is declared with
/// the widest integer type seen across its rows; on import the serializer
/// re-derives each value's exact wire width from the value itself.
/// </remarks>
public static class SqliteMarshalFunctionExporter
{
    public static void Export(string connectionString, MarshalFunction function, FieldMappings? fieldMappings = null)
    {
        using var connection = new SqliteConnection(connectionString);
        connection.Open();
        Export(connection, function, fieldMappings);
    }

    public static void Export(SqliteConnection connection, MarshalFunction function, FieldMappings? fieldMappings = null)
    {
        using var transaction = connection.BeginTransaction();

        EnsureForeignKeys(connection, transaction);
        DropAllTables(connection, transaction);
        EnsureFunctionMetadataTable(connection, transaction);
        EnsureDataSetFieldsTable(connection, transaction);

        // Pre-scan the whole function so every column is declared with the widest
        // type present in its rows (the wire width varies per value).
        var schema = new Dictionary<string, Dictionary<string, string>>(StringComparer.OrdinalIgnoreCase);
        var metadataColumns = CollectRowColumns(function.Rows, MarshalSqliteSchema.FunctionMetadataTable, schema, fieldMappings);

        var createdTables = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        var functionId = InsertFunctionMetadata(connection, transaction, function, metadataColumns);

        var ordinal = 0;

        foreach (var (fieldName, marshalObject) in function.Rows)
        {
            if (marshalObject.Type == FieldType.DataSet)
            {
                ExportDataSet(connection, transaction, functionId, null, null, fieldName, marshalObject, ordinal, schema, createdTables, fieldMappings);
            }

            ordinal++;
        }

        transaction.Commit();
    }

    private static void EnsureForeignKeys(SqliteConnection connection, SqliteTransaction transaction)
    {
        using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = "PRAGMA foreign_keys = ON;";
        command.ExecuteNonQuery();
    }

    private static void DropAllTables(SqliteConnection connection, SqliteTransaction transaction)
    {
        using var disableFk = connection.CreateCommand();
        disableFk.Transaction = transaction;
        disableFk.CommandText = "PRAGMA foreign_keys = OFF;";
        disableFk.ExecuteNonQuery();

        var tableNames = new List<string>();
        using (var tablesCommand = connection.CreateCommand())
        {
            tablesCommand.Transaction = transaction;
            tablesCommand.CommandText =
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%';";

            using var reader = tablesCommand.ExecuteReader();
            while (reader.Read())
            {
                tableNames.Add(reader.GetString(0));
            }
        }

        // Drop children before parents so the schema is recreated deterministically.
        foreach (var tableName in tableNames)
        {
            using var dropCommand = connection.CreateCommand();
            dropCommand.Transaction = transaction;
            dropCommand.CommandText = $"DROP TABLE IF EXISTS {MarshalSqliteSchema.QuoteIdentifier(tableName)};";
            dropCommand.ExecuteNonQuery();
        }

        using var enableFk = connection.CreateCommand();
        enableFk.Transaction = transaction;
        enableFk.CommandText = "PRAGMA foreign_keys = ON;";
        enableFk.ExecuteNonQuery();
    }

    private static void EnsureFunctionMetadataTable(SqliteConnection connection, SqliteTransaction transaction)
    {
        using var createCommand = connection.CreateCommand();
        createCommand.Transaction = transaction;
        createCommand.CommandText = $@"
CREATE TABLE {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionMetadataTable)} (
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionIdColumn)} INTEGER PRIMARY KEY AUTOINCREMENT,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.VersionColumn)} INTEGER NOT NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionColumn)} INTEGER NOT NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionNameColumn)} TEXT NULL
);
";
        createCommand.ExecuteNonQuery();
    }

    private static void EnsureDataSetFieldsTable(SqliteConnection connection, SqliteTransaction transaction)
    {
        using var createCommand = connection.CreateCommand();
        createCommand.Transaction = transaction;
        createCommand.CommandText = $@"
CREATE TABLE {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.DataSetFieldsTable)} (
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionIdColumn)} INTEGER NOT NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.TableNameColumn)} TEXT NOT NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.ParentTableColumn)} TEXT NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.ParentRowIdColumn)} INTEGER NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FieldOrdinalColumn)} INTEGER NOT NULL,
    UNIQUE ({MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.TableNameColumn)},
            {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.ParentTableColumn)},
            {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.ParentRowIdColumn)},
            {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FieldOrdinalColumn)})
);
";
        createCommand.ExecuteNonQuery();
    }

    /// <summary>
    /// Collects the widest declared type per column across all rows of one table.
    /// Data-set fields are skipped here and collected into their own tables.
    /// </summary>
    private static Dictionary<string, string> CollectRowColumns(
        IEnumerable<KeyValuePair<string, MarshalObject>> entries,
        string tableName,
        Dictionary<string, Dictionary<string, string>> schema,
        FieldMappings? fieldMappings)
    {
        var columns = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);

        foreach (var (fieldName, marshalObject) in entries)
        {
            if (marshalObject.Type == FieldType.DataSet)
            {
                CollectDataSetColumns(fieldName, marshalObject, schema, fieldMappings);
                continue;
            }

            AddSchemaColumn(columns, tableName, fieldName, marshalObject, fieldMappings);
        }

        return columns;
    }

    private static void CollectDataSetColumns(
        string fieldName,
        MarshalObject marshalObject,
        Dictionary<string, Dictionary<string, string>> schema,
        FieldMappings? fieldMappings)
    {
        var tableName = MarshalSqliteSchema.NormalizeIdentifier(fieldName);
        var columns = schema.GetValueOrDefault(tableName);

        foreach (var row in (IList<Dictionary<string, MarshalObject>>)marshalObject.Value)
        {
            if (columns == null)
            {
                columns = [];
                schema[tableName] = columns;
            }

            foreach (var (columnNameRaw, value) in row)
            {
                if (value.Type == FieldType.DataSet)
                {
                    CollectDataSetColumns(columnNameRaw, value, schema, fieldMappings);
                    continue;
                }

                var columnName = MarshalSqliteSchema.NormalizeIdentifier(columnNameRaw);
                if (MarshalSqliteSchema.DataSetTableColumns.Contains(columnName, StringComparer.OrdinalIgnoreCase))
                {
                    throw new InvalidOperationException(
                        $"Field name \"{columnNameRaw}\" normalizes to reserved column \"{columnName}\" in table \"{tableName}\".");
                }

                AddColumnType(columns, columnName, columnNameRaw, value, fieldMappings);
            }
        }
    }

    private static void AddSchemaColumn(
        Dictionary<string, string> columns,
        string tableName,
        string fieldName,
        MarshalObject marshalObject,
        FieldMappings? fieldMappings)
    {
        var columnName = MarshalSqliteSchema.NormalizeIdentifier(fieldName);

        if (MarshalSqliteSchema.FunctionMetadataColumns.Contains(columnName, StringComparer.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException(
                $"Field name \"{fieldName}\" normalizes to reserved column \"{columnName}\" in table \"{tableName}\".");
        }

        AddColumnType(columns, columnName, fieldName, marshalObject, fieldMappings);
    }

    private static void AddColumnType(
        Dictionary<string, string> columns,
        string columnName,
        string fieldName,
        MarshalObject marshalObject,
        FieldMappings? fieldMappings)
    {
        var declaredType = MarshalSqliteSchema.GetDeclaredType(marshalObject, fieldName, fieldMappings);
        columns.TryGetValue(columnName, out var existing);
        columns[columnName] = existing is null ? declaredType : Widen(existing, declaredType);
    }

    /// <summary>
    /// Widens integer types (BYTE → SHORT → INT → LONG) so a column can hold any
    /// value of its rows; the serializer re-derives exact wire widths on import.
    /// Non-promotable kinds keep the first-seen type.
    /// </summary>
    private static string Widen(string existing, string candidate)
    {
        if (existing == candidate)
            return existing;

        var rank = new[] { "BYTE", "SHORT", "INT", "LONG" };
        var existingRank = Array.IndexOf(rank, existing);
        var candidateRank = Array.IndexOf(rank, candidate);

        return existingRank >= 0 && candidateRank >= 0
            ? rank[Math.Max(existingRank, candidateRank)]
            : existing;
    }

    private static long InsertFunctionMetadata(
        SqliteConnection connection,
        SqliteTransaction transaction,
        MarshalFunction function,
        IReadOnlyDictionary<string, string> metadataColumns)
    {
        EnsureColumns(connection, transaction, MarshalSqliteSchema.FunctionMetadataTable, metadataColumns);

        var values = new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase);

        foreach (var (fieldName, marshalObject) in function.Rows)
        {
            if (marshalObject.Type == FieldType.DataSet)
                continue;

            var columnName = MarshalSqliteSchema.NormalizeIdentifier(fieldName);
            values[columnName] = MarshalSqliteSchema.ToValue(marshalObject);
        }

        var insertColumns = new List<string>(MarshalSqliteSchema.FunctionMetadataColumns);
        var insertValues = insertColumns.Select(c => "$" + c).ToList();

        foreach (var columnName in values.Keys)
        {
            insertColumns.Add(columnName);
            insertValues.Add("$" + columnName);
        }

        using var insertCommand = connection.CreateCommand();
        insertCommand.Transaction = transaction;
        insertCommand.CommandText = $@"
INSERT INTO {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionMetadataTable)} ({MarshalSqliteSchema.JoinIdentifiers(insertColumns)})
VALUES ({string.Join(", ", insertValues)});
SELECT last_insert_rowid();
";

        insertCommand.Parameters.AddWithValue("$" + MarshalSqliteSchema.FunctionIdColumn, DBNull.Value);
        insertCommand.Parameters.AddWithValue("$" + MarshalSqliteSchema.VersionColumn, (int)function.Version);
        insertCommand.Parameters.AddWithValue("$" + MarshalSqliteSchema.FunctionColumn, unchecked((long)function.Function));
        insertCommand.Parameters.AddWithValue("$" + MarshalSqliteSchema.FunctionNameColumn, (object?)function.FunctionName ?? DBNull.Value);

        foreach (var (columnName, value) in values)
        {
            insertCommand.Parameters.AddWithValue("$" + columnName, value ?? DBNull.Value);
        }

        var result = insertCommand.ExecuteScalar();
        return result is long id ? id : Convert.ToInt64(result, System.Globalization.CultureInfo.InvariantCulture);
    }

    private static void ExportDataSet(
        SqliteConnection connection,
        SqliteTransaction transaction,
        long functionId,
        long? parentRowId,
        string? parentTable,
        string fieldName,
        MarshalObject marshalObject,
        int fieldOrdinal,
        Dictionary<string, Dictionary<string, string>> schema,
        HashSet<string> createdTables,
        FieldMappings? fieldMappings)
    {
        var tableName = MarshalSqliteSchema.NormalizeIdentifier(fieldName);
        var rows = (IList<Dictionary<string, MarshalObject>>)marshalObject.Value;

        EnsureDataSetTable(connection, transaction, tableName);

        if (createdTables.Add(tableName) && schema.TryGetValue(tableName, out var columns))
        {
            EnsureColumns(connection, transaction, tableName, columns);
        }

        if (rows.Count == 0)
        {
            // A data set without rows stores no per-row ordinals, so record its
            // position in the parent row here; otherwise the round trip would
            // lose the field entirely.
            InsertDataSetField(connection, transaction, functionId, tableName, parentTable, parentRowId, fieldOrdinal);
            return;
        }

        foreach (var row in rows)
        {
            var values = new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase);
            var nestedDataSets = new List<(string FieldName, MarshalObject Value, int Ordinal)>();

            var ordinal = 0;

            foreach (var (columnNameRaw, value) in row)
            {
                if (value.Type == FieldType.DataSet)
                {
                    nestedDataSets.Add((columnNameRaw, value, ordinal));
                }
                else
                {
                    values[MarshalSqliteSchema.NormalizeIdentifier(columnNameRaw)] = MarshalSqliteSchema.ToValue(value);
                }

                ordinal++;
            }

            var rowId = InsertDataSetRow(connection, transaction, tableName, functionId, parentRowId, parentTable, fieldOrdinal, values,
                ComputeEntryOrder(row, tableName, schema));

            foreach (var (nestedFieldName, nestedValue, nestedOrdinal) in nestedDataSets)
            {
                ExportDataSet(connection, transaction, functionId, rowId, tableName, nestedFieldName, nestedValue, nestedOrdinal, schema, createdTables, fieldMappings);
            }
        }
    }

    private static void EnsureDataSetTable(SqliteConnection connection, SqliteTransaction transaction, string tableName)
    {
        using var createCommand = connection.CreateCommand();
        createCommand.Transaction = transaction;
        createCommand.CommandText = $@"
CREATE TABLE IF NOT EXISTS {MarshalSqliteSchema.QuoteIdentifier(tableName)} (
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.RowIdColumn)} INTEGER PRIMARY KEY AUTOINCREMENT,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionIdColumn)} INTEGER NOT NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.ParentRowIdColumn)} INTEGER NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.ParentTableColumn)} TEXT NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FieldOrdinalColumn)} INTEGER NOT NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.EntryOrderColumn)} TEXT NULL,
    FOREIGN KEY ({MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionIdColumn)})
        REFERENCES {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionMetadataTable)}({MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionIdColumn)}) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS {MarshalSqliteSchema.QuoteIdentifier("ix_" + tableName + "_function_id")}
    ON {MarshalSqliteSchema.QuoteIdentifier(tableName)}({MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionIdColumn)});

CREATE INDEX IF NOT EXISTS {MarshalSqliteSchema.QuoteIdentifier("ix_" + tableName + "_parent_row_id")}
    ON {MarshalSqliteSchema.QuoteIdentifier(tableName)}({MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.ParentRowIdColumn)});
";
        createCommand.ExecuteNonQuery();
    }

    private static void InsertDataSetField(
        SqliteConnection connection,
        SqliteTransaction transaction,
        long functionId,
        string tableName,
        string? parentTable,
        long? parentRowId,
        int fieldOrdinal)
    {
        using var insertCommand = connection.CreateCommand();
        insertCommand.Transaction = transaction;
        insertCommand.CommandText = $@"
INSERT OR IGNORE INTO {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.DataSetFieldsTable)}
    ({MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionIdColumn)},
     {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.TableNameColumn)},
     {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.ParentTableColumn)},
     {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.ParentRowIdColumn)},
     {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FieldOrdinalColumn)})
VALUES ($function_id, $table_name, $parent_table, $parent_row_id, $field_ordinal);
";

        insertCommand.Parameters.AddWithValue("$function_id", functionId);
        insertCommand.Parameters.AddWithValue("$table_name", tableName);
        insertCommand.Parameters.AddWithValue("$parent_table", (object?)parentTable ?? DBNull.Value);
        insertCommand.Parameters.AddWithValue("$parent_row_id", parentRowId.HasValue ? parentRowId.Value : DBNull.Value);
        insertCommand.Parameters.AddWithValue("$field_ordinal", fieldOrdinal);
        insertCommand.ExecuteNonQuery();
    }

    /// <summary>
    /// Returns the row's data column sequence (normalized names, entry order)
    /// when it deviates from the table's first-seen column order; otherwise null.
    /// </summary>
    private static string? ComputeEntryOrder(
        IReadOnlyDictionary<string, MarshalObject> row,
        string tableName,
        Dictionary<string, Dictionary<string, string>> schema)
    {
        if (!schema.TryGetValue(tableName, out var columns))
            return null;

        var present = new List<string>();
        foreach (var (fieldName, value) in row)
        {
            if (value.Type != FieldType.DataSet)
                present.Add(MarshalSqliteSchema.NormalizeIdentifier(fieldName));
        }

        var expected = columns.Keys.Where(present.Contains).ToList();
        return present.SequenceEqual(expected) ? null : string.Join(",", present);
    }

    private static long InsertDataSetRow(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string tableName,
        long functionId,
        long? parentRowId,
        string? parentTable,
        int fieldOrdinal,
        IReadOnlyDictionary<string, object?> values,
        string? entryOrder)
    {
        var insertColumns = new List<string>(MarshalSqliteSchema.DataSetTableColumns);
        var insertValues = insertColumns.Select(c => "$" + c).ToList();

        foreach (var column in values.Keys)
        {
            insertColumns.Add(column);
            insertValues.Add("$" + column);
        }

        using var insertCommand = connection.CreateCommand();
        insertCommand.Transaction = transaction;
        insertCommand.CommandText = $@"
INSERT INTO {MarshalSqliteSchema.QuoteIdentifier(tableName)} ({MarshalSqliteSchema.JoinIdentifiers(insertColumns)})
VALUES ({string.Join(", ", insertValues)});
SELECT last_insert_rowid();
";

        insertCommand.Parameters.AddWithValue("$" + MarshalSqliteSchema.RowIdColumn, DBNull.Value);
        insertCommand.Parameters.AddWithValue("$" + MarshalSqliteSchema.FunctionIdColumn, functionId);
        insertCommand.Parameters.AddWithValue("$" + MarshalSqliteSchema.ParentRowIdColumn, parentRowId.HasValue ? parentRowId.Value : DBNull.Value);
        insertCommand.Parameters.AddWithValue("$" + MarshalSqliteSchema.ParentTableColumn, (object?)parentTable ?? DBNull.Value);
        insertCommand.Parameters.AddWithValue("$" + MarshalSqliteSchema.FieldOrdinalColumn, fieldOrdinal);
        insertCommand.Parameters.AddWithValue("$" + MarshalSqliteSchema.EntryOrderColumn, (object?)entryOrder ?? DBNull.Value);

        foreach (var (column, value) in values)
        {
            insertCommand.Parameters.AddWithValue("$" + column, value ?? DBNull.Value);
        }

        var result = insertCommand.ExecuteScalar();
        return result is long id ? id : Convert.ToInt64(result, System.Globalization.CultureInfo.InvariantCulture);
    }

    private static void EnsureColumns(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string tableName,
        IEnumerable<KeyValuePair<string, string>> columns)
    {
        var existingColumns = GetExistingColumns(connection, transaction, tableName);

        foreach (var (columnName, declaredType) in columns)
        {
            if (existingColumns.Contains(columnName))
                continue;

            using var alterCommand = connection.CreateCommand();
            alterCommand.Transaction = transaction;
            alterCommand.CommandText = $@"
ALTER TABLE {MarshalSqliteSchema.QuoteIdentifier(tableName)}
ADD COLUMN {MarshalSqliteSchema.QuoteIdentifier(columnName)} {declaredType};
";
            alterCommand.ExecuteNonQuery();
        }
    }

    private static HashSet<string> GetExistingColumns(SqliteConnection connection, SqliteTransaction transaction, string tableName)
    {
        using var pragmaCommand = connection.CreateCommand();
        pragmaCommand.Transaction = transaction;
        pragmaCommand.CommandText = $"PRAGMA table_info({MarshalSqliteSchema.QuoteIdentifier(tableName)});";

        using var reader = pragmaCommand.ExecuteReader();
        var columns = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        while (reader.Read())
        {
            var name = reader.GetString(1);
            columns.Add(name);
        }

        return columns;
    }
}
