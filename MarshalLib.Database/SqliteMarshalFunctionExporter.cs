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

        DropAndRecreateSchema(connection, transaction);

        var context = new ExportContext(connection, transaction, fieldMappings);

        // Pre-scan the whole function so every column is declared with the widest
        // type present in its rows (the wire width varies per value).
        var metadataColumns = context.CollectFunctionColumns(function.Rows);

        var functionId = InsertFunctionMetadata(connection, transaction, function, metadataColumns, context);

        var ordinal = 0;

        foreach (var (fieldName, marshalObject) in function.Rows)
        {
            if (marshalObject.Type == FieldType.DataSet)
            {
                context.GetDataSetWriter(fieldName).Write(functionId, parentRowId: null, parentTable: null, marshalObject, ordinal);
            }

            ordinal++;
        }

        CreateAllDataSetTables(context, fieldMappings);

        transaction.Commit();
    }

    /// <summary>
    /// Creates a table for every DATA_SET field in the field mappings, so the
    /// exported database contains the game's complete dataset dictionary. Fields
    /// absent from the file become empty tables (e.g. DATA_SET_BOTS); empty tables
    /// are inert on import (no rows, no entries), so the marshal round trip stays
    /// byte-exact.
    /// </summary>
    private static void CreateAllDataSetTables(ExportContext context, FieldMappings? fieldMappings)
    {
        if (fieldMappings is null)
            return;

        foreach (var field in fieldMappings.Fields)
        {
            if (field.Type == FieldType.DataSet)
            {
                context.GetDataSetWriter(field.Name);
            }
        }
    }

    private static void DropAndRecreateSchema(SqliteConnection connection, SqliteTransaction transaction)
    {
        EnsureForeignKeys(connection, transaction);
        DropAllTables(connection, transaction);
        EnsureFunctionMetadataTable(connection, transaction);
        EnsureDataSetFieldsTable(connection, transaction);
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

    private static long InsertFunctionMetadata(
        SqliteConnection connection,
        SqliteTransaction transaction,
        MarshalFunction function,
        IReadOnlyDictionary<string, string> metadataColumns,
        ExportContext context)
    {
        EnsureColumns(connection, transaction, MarshalSqliteSchema.FunctionMetadataTable, metadataColumns);

        var values = new Dictionary<string, object?>(StringComparer.OrdinalIgnoreCase);

        foreach (var (fieldName, marshalObject) in function.Rows)
        {
            if (marshalObject.Type == FieldType.DataSet)
                continue;

            values[context.Normalize(fieldName)] = MarshalSqliteSchema.ToValue(marshalObject);
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

    /// <summary>
    /// Adds missing columns to an existing table in schema order, so the column
    /// order matches the first-seen marshal entry order.
    /// </summary>
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

    /// <summary>
    /// Shared state for one export: the open transaction, the per-table schema
    /// collected by the pre-scan, the field-name normalization cache, and the
    /// lazily created per-table writers.
    /// </summary>
    private sealed class ExportContext
    {
        public ExportContext(SqliteConnection connection, SqliteTransaction transaction, FieldMappings? fieldMappings)
        {
            Connection = connection;
            Transaction = transaction;
            FieldMappings = fieldMappings;
        }

        public SqliteConnection Connection { get; }

        public SqliteTransaction Transaction { get; }

        public FieldMappings? FieldMappings { get; }

        /// <summary>Table name -> column name -> declared column type (insertion order = column order).</summary>
        public Dictionary<string, Dictionary<string, string>> Schema { get; } = new(StringComparer.OrdinalIgnoreCase);

        /// <summary>Field name -> normalized column name (field names repeat across rows).</summary>
        private readonly Dictionary<string, string> _normalizedNames = new(StringComparer.OrdinalIgnoreCase);

        private readonly Dictionary<string, DataSetTableWriter> _dataSetWriters = new(StringComparer.OrdinalIgnoreCase);

        public string Normalize(string fieldName)
        {
            if (!_normalizedNames.TryGetValue(fieldName, out var normalized))
            {
                normalized = MarshalSqliteSchema.NormalizeIdentifier(fieldName);
                _normalizedNames[fieldName] = normalized;
            }

            return normalized;
        }

        /// <summary>
        /// Returns the writer for a data-set field's table, creating the table
        /// and its prepared insert command on first use.
        /// </summary>
        public DataSetTableWriter GetDataSetWriter(string fieldName)
        {
            var tableName = Normalize(fieldName);

            if (!_dataSetWriters.TryGetValue(tableName, out var writer))
            {
                Schema.TryGetValue(tableName, out var schema);
                writer = new DataSetTableWriter(this, tableName, schema);
                _dataSetWriters[tableName] = writer;
            }

            return writer;
        }

        /// <summary>
        /// Collects the widest declared type per column across all rows of the
        /// function metadata table. Data-set fields are skipped here and
        /// collected into their own per-table schemas in <see cref="Schema"/>.
        /// </summary>
        public Dictionary<string, string> CollectFunctionColumns(IEnumerable<KeyValuePair<string, MarshalObject>> entries)
        {
            var columns = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);

            foreach (var (fieldName, marshalObject) in entries)
            {
                if (marshalObject.Type == FieldType.DataSet)
                {
                    CollectDataSetColumns(fieldName, marshalObject);
                    continue;
                }

                AddSchemaColumn(columns, MarshalSqliteSchema.FunctionMetadataTable, fieldName, marshalObject);
            }

            return columns;
        }

        /// <summary>Collects the schema of one data-set table, recursing into nested data sets.</summary>
        private void CollectDataSetColumns(string fieldName, MarshalObject marshalObject)
        {
            var tableName = Normalize(fieldName);

            if (!Schema.TryGetValue(tableName, out var columns))
            {
                columns = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
                Schema[tableName] = columns;
            }

            foreach (var row in (IList<Dictionary<string, MarshalObject>>)marshalObject.Value)
            {
                foreach (var (columnNameRaw, value) in row)
                {
                    if (value.Type == FieldType.DataSet)
                    {
                        CollectDataSetColumns(columnNameRaw, value);
                        continue;
                    }

                    var columnName = Normalize(columnNameRaw);

                    if (MarshalSqliteSchema.DataSetTableColumns.Contains(columnName, StringComparer.OrdinalIgnoreCase))
                    {
                        throw new InvalidOperationException(
                            $"Field name \"{columnNameRaw}\" normalizes to reserved column \"{columnName}\" in table \"{tableName}\".");
                    }

                    AddColumnType(columns, columnName, columnNameRaw, value);
                }
            }
        }

        private void AddSchemaColumn(Dictionary<string, string> columns, string tableName, string fieldName, MarshalObject marshalObject)
        {
            var columnName = Normalize(fieldName);

            if (MarshalSqliteSchema.FunctionMetadataColumns.Contains(columnName, StringComparer.OrdinalIgnoreCase))
            {
                throw new InvalidOperationException(
                    $"Field name \"{fieldName}\" normalizes to reserved column \"{columnName}\" in table \"{tableName}\".");
            }

            AddColumnType(columns, columnName, fieldName, marshalObject);
        }

        private void AddColumnType(Dictionary<string, string> columns, string columnName, string fieldName, MarshalObject marshalObject)
        {
            var declaredType = MarshalSqliteSchema.GetDeclaredType(marshalObject, fieldName, FieldMappings);
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

            var existingRank = Array.IndexOf(IntegerTypeRank, existing);
            var candidateRank = Array.IndexOf(IntegerTypeRank, candidate);

            if (existingRank >= 0 && candidateRank >= 0)
                return IntegerTypeRank[Math.Max(existingRank, candidateRank)];

            // For strings the lossless UTF-16 encoding wins over the
            // single-byte ones; the importer downgrades ASCII-able values back.
            existingRank = Array.IndexOf(StringTypeRank, existing);
            candidateRank = Array.IndexOf(StringTypeRank, candidate);

            return existingRank >= 0 && candidateRank >= 0
                ? StringTypeRank[Math.Max(existingRank, candidateRank)]
                : existing;
        }

        private static readonly string[] IntegerTypeRank = ["BYTE", "SHORT", "INT", "LONG"];
        private static readonly string[] StringTypeRank = ["TEXT", "TEXT_ASCII", "TEXT_UTF8", "TEXT_UTF32", "TEXT_UTF16"];
    }

    /// <summary>
    /// Writes one data-set table. Owns the table's schema columns, its prepared
    /// INSERT command with a fixed parameter per column, the column-name →
    /// parameter-index map, the per-table row-id counter, and the entry-order
    /// logic. Created lazily on the table's first occurrence and reused for all
    /// rows of the table, wherever they appear in the function.
    /// </summary>
    private sealed class DataSetTableWriter
    {
        private readonly ExportContext _context;

        /// <summary>Normalized table name.</summary>
        public string TableName { get; }

        /// <summary>Data columns in first-seen order (the table's entry order).</summary>
        private readonly List<string> _schemaColumns;

        private readonly SqliteCommand _insertCommand;
        private readonly SqliteParameter[] _parameters;
        private readonly Dictionary<string, int> _dataColumnIndex;

        /// <summary>Per-row scratch: normalized names of the row's data fields, in entry order.</summary>
        private readonly List<string> _presentColumns = [];

        /// <summary>Per-row scratch: data parameters written for the current row, reset after each insert.</summary>
        private readonly List<int> _touchedParameters = [];

        /// <summary>Row ids only need to be unique per table and rows are inserted in order.</summary>
        private long _nextRowId = 1;

        private static readonly int RowIdParameter = Array.IndexOf(MarshalSqliteSchema.DataSetTableColumns, MarshalSqliteSchema.RowIdColumn);
        private static readonly int FunctionIdParameter = Array.IndexOf(MarshalSqliteSchema.DataSetTableColumns, MarshalSqliteSchema.FunctionIdColumn);
        private static readonly int ParentRowIdParameter = Array.IndexOf(MarshalSqliteSchema.DataSetTableColumns, MarshalSqliteSchema.ParentRowIdColumn);
        private static readonly int ParentTableParameter = Array.IndexOf(MarshalSqliteSchema.DataSetTableColumns, MarshalSqliteSchema.ParentTableColumn);
        private static readonly int FieldOrdinalParameter = Array.IndexOf(MarshalSqliteSchema.DataSetTableColumns, MarshalSqliteSchema.FieldOrdinalColumn);
        private static readonly int EntryOrderParameter = Array.IndexOf(MarshalSqliteSchema.DataSetTableColumns, MarshalSqliteSchema.EntryOrderColumn);

        public DataSetTableWriter(ExportContext context, string tableName, Dictionary<string, string>? schema)
        {
            _context = context;
            TableName = tableName;
            _schemaColumns = schema?.Keys.ToList() ?? [];

            CreateTable(schema);

            var columns = new List<string>(MarshalSqliteSchema.DataSetTableColumns);
            columns.AddRange(_schemaColumns);

            _insertCommand = context.Connection.CreateCommand();
            _insertCommand.Transaction = context.Transaction;
            _insertCommand.CommandText = $@"
INSERT INTO {MarshalSqliteSchema.QuoteIdentifier(tableName)} ({MarshalSqliteSchema.JoinIdentifiers(columns)})
VALUES ({string.Join(", ", columns.Select(c => "$" + c))});
";

            _parameters = new SqliteParameter[columns.Count];
            _dataColumnIndex = new Dictionary<string, int>(_schemaColumns.Count, StringComparer.OrdinalIgnoreCase);

            for (var i = 0; i < columns.Count; i++)
            {
                _parameters[i] = _insertCommand.Parameters.AddWithValue("$" + columns[i], DBNull.Value);

                if (i >= MarshalSqliteSchema.DataSetTableColumns.Length)
                    _dataColumnIndex[columns[i]] = i;
            }
        }

        /// <summary>
        /// Writes one data-set value (all of its rows) with its position within
        /// the parent row. Nested data sets are written by their own writers.
        /// </summary>
        public void Write(long functionId, long? parentRowId, string? parentTable, MarshalObject marshalObject, int fieldOrdinal)
        {
            var rows = (IList<Dictionary<string, MarshalObject>>)marshalObject.Value;

            if (rows.Count == 0)
            {
                // A data set without rows stores no per-row ordinals, so record its
                // position in the parent row here; otherwise the round trip would
                // lose the field entirely.
                InsertDataSetField(functionId, parentTable, parentRowId, fieldOrdinal);
                return;
            }

            foreach (var row in rows)
            {
                WriteRow(functionId, parentRowId, parentTable, row, fieldOrdinal);
            }
        }

        private void WriteRow(long functionId, long? parentRowId, string? parentTable, Dictionary<string, MarshalObject> row, int fieldOrdinal)
        {
            // Only allocated when the row actually nests data sets (the common
            // case for rows is scalar fields only).
            List<(string FieldName, MarshalObject Value, int Ordinal)>? nestedDataSets = null;

            _presentColumns.Clear();
            _touchedParameters.Clear();

            var ordinal = 0;

            // Walk the row once: data values are converted and written straight
            // into their fixed parameters via the column-name → index map, and
            // the entry-order sequence is recorded at the same time.
            foreach (var (fieldName, value) in row)
            {
                if (value.Type == FieldType.DataSet)
                {
                    (nestedDataSets ??= []).Add((fieldName, value, ordinal));
                }
                else
                {
                    var columnName = _context.Normalize(fieldName);
                    _presentColumns.Add(columnName);

                    if (_dataColumnIndex.TryGetValue(columnName, out var parameterIndex))
                    {
                        var converted = MarshalSqliteSchema.ToValue(value);
                        _parameters[parameterIndex].Value = converted ?? DBNull.Value;
                        _touchedParameters.Add(parameterIndex);
                    }
                }

                ordinal++;
            }

            var rowId = _nextRowId++;

            _parameters[RowIdParameter].Value = rowId;
            _parameters[FunctionIdParameter].Value = functionId;
            _parameters[ParentRowIdParameter].Value = parentRowId.HasValue ? parentRowId.Value : DBNull.Value;
            _parameters[ParentTableParameter].Value = (object?)parentTable ?? DBNull.Value;
            _parameters[FieldOrdinalParameter].Value = fieldOrdinal;
            _parameters[EntryOrderParameter].Value = (object?)ComputeEntryOrder() ?? DBNull.Value;

            _insertCommand.ExecuteNonQuery();

            // Reset the data parameters written above so the next row starts
            // from all-NULL; the reserved parameters are overwritten every row.
            foreach (var parameterIndex in _touchedParameters)
            {
                _parameters[parameterIndex].Value = DBNull.Value;
            }

            if (nestedDataSets != null)
            {
                foreach (var (nestedFieldName, nestedValue, nestedOrdinal) in nestedDataSets)
                {
                    _context.GetDataSetWriter(nestedFieldName).Write(functionId, rowId, TableName, nestedValue, nestedOrdinal);
                }
            }
        }

        /// <summary>
        /// Returns the row's data column sequence (normalized names, entry order)
        /// when it deviates from the table's first-seen column order; otherwise null.
        /// </summary>
        private string? ComputeEntryOrder()
        {
            // Fast path: the common case is a uniform row matching the table order.
            if (_presentColumns.Count == _schemaColumns.Count)
            {
                var matches = true;

                for (var i = 0; i < _presentColumns.Count; i++)
                {
                    if (_schemaColumns[i] != _presentColumns[i])
                    {
                        matches = false;
                        break;
                    }
                }

                if (matches)
                    return null;
            }

            var presentSet = new HashSet<string>(_presentColumns, StringComparer.OrdinalIgnoreCase);
            var expected = new List<string>(_presentColumns.Count);

            foreach (var column in _schemaColumns)
            {
                if (presentSet.Contains(column))
                    expected.Add(column);
            }

            return _presentColumns.SequenceEqual(expected) ? null : string.Join(",", _presentColumns);
        }

        private void CreateTable(Dictionary<string, string>? schema)
        {
            using var createCommand = _context.Connection.CreateCommand();
            createCommand.Transaction = _context.Transaction;
            createCommand.CommandText = $@"
CREATE TABLE IF NOT EXISTS {MarshalSqliteSchema.QuoteIdentifier(TableName)} (
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.RowIdColumn)} INTEGER PRIMARY KEY AUTOINCREMENT,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionIdColumn)} INTEGER NOT NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.ParentRowIdColumn)} INTEGER NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.ParentTableColumn)} TEXT NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FieldOrdinalColumn)} INTEGER NOT NULL,
    {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.EntryOrderColumn)} TEXT NULL,
    FOREIGN KEY ({MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionIdColumn)})
        REFERENCES {MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionMetadataTable)}({MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionIdColumn)}) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS {MarshalSqliteSchema.QuoteIdentifier("ix_" + TableName + "_function_id")}
    ON {MarshalSqliteSchema.QuoteIdentifier(TableName)}({MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.FunctionIdColumn)});

CREATE INDEX IF NOT EXISTS {MarshalSqliteSchema.QuoteIdentifier("ix_" + TableName + "_parent_row_id")}
    ON {MarshalSqliteSchema.QuoteIdentifier(TableName)}({MarshalSqliteSchema.QuoteIdentifier(MarshalSqliteSchema.ParentRowIdColumn)});
";
            createCommand.ExecuteNonQuery();

            if (schema != null)
            {
                EnsureColumns(_context.Connection, _context.Transaction, TableName, schema);
            }
        }

        private void InsertDataSetField(long functionId, string? parentTable, long? parentRowId, int fieldOrdinal)
        {
            using var insertCommand = _context.Connection.CreateCommand();
            insertCommand.Transaction = _context.Transaction;
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
            insertCommand.Parameters.AddWithValue("$table_name", TableName);
            insertCommand.Parameters.AddWithValue("$parent_table", (object?)parentTable ?? DBNull.Value);
            insertCommand.Parameters.AddWithValue("$parent_row_id", parentRowId.HasValue ? parentRowId.Value : DBNull.Value);
            insertCommand.Parameters.AddWithValue("$field_ordinal", fieldOrdinal);
            insertCommand.ExecuteNonQuery();
        }
    }
}
