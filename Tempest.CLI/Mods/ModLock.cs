namespace Tempest.CLI.Mods;

/// <summary>
/// Serialises changes to one game folder's mods across CLI processes: the launcher runs several
/// installs at once, and each reads and rewrites mods.json, game files and INIs. Held as an
/// exclusive handle on <c>.tempest/mods/mods.lock</c>, which the OS releases if the process dies.
/// Not re-entrant across instances: one process takes it through a single <see cref="ModLock"/>.
/// </summary>
public sealed class ModLock(string gamePath) : IDisposable
{
    private static readonly TimeSpan Timeout = TimeSpan.FromMinutes(5);

    private FileStream? _handle;

    public static async Task<ModLock> AcquireAsync(string gamePath)
    {
        var modLock = new ModLock(gamePath);
        await modLock.EnterAsync();
        return modLock;
    }

    /// <summary>Waits for the lock; once held it stays held until disposed, and entering again does nothing.</summary>
    public async Task EnterAsync()
    {
        if (_handle != null) return;
        var path = Path.Combine(TempestPathUtility.GetLocalModsDirectory(GameFolderResolver.Resolve(gamePath)), "mods.lock");
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        var deadline = DateTime.UtcNow + Timeout;
        while (true)
        {
            try
            {
                _handle = new FileStream(path, FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None);
                return;
            }
            catch (IOException) when (DateTime.UtcNow < deadline)
            {
                await Task.Delay(50);
            }
        }
    }

    public void Dispose()
    {
        _handle?.Dispose();
        _handle = null;
    }
}
