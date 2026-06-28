using Microsoft.Data.Sqlite;

namespace Tempest.Services.Persistence;

/// <summary>
/// Opens a <see cref="SqliteConnection"/> against the connection string resolved from
/// <c>ConnectionStrings:Default</c>. Registered as a scoped service so connections are
/// reused within a single request and disposed at scope teardown.
/// </summary>
public sealed class SqliteConnectionFactory(IConfiguration configuration) : IDisposable, IAsyncDisposable
{
    private readonly string _connectionString =
        configuration.GetConnectionString("Default")
        ?? throw new InvalidOperationException("ConnectionStrings:Default is not configured.");

    private SqliteConnection? _connection;

    public SqliteConnection Create()
    {
        if (_connection is null)
        {
            _connection = new SqliteConnection(_connectionString);
            _connection.Open();
        }

        return _connection;
    }

    public void Dispose() => _connection?.Dispose();

    public ValueTask DisposeAsync()
    {
        if (_connection is null)
        {
            return ValueTask.CompletedTask;
        }

        return _connection.DisposeAsync();
    }
}
