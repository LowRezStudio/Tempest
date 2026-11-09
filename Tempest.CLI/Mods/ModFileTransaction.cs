namespace Tempest.CLI.Mods;

internal static class ModFileTransaction
{
    internal static async Task RunAsync(IEnumerable<string> paths, Func<Task> change)
    {
        var directory = Path.Combine(Path.GetTempPath(), $"tempest-mod-change-{Guid.NewGuid():N}");
        Directory.CreateDirectory(directory);
        var snapshots = new List<(string Path, string? Backup)>();
        var keepBackups = false;
        try
        {
            foreach (var path in paths.Select(Path.GetFullPath).Distinct(StringComparer.OrdinalIgnoreCase))
            {
                string? backup = null;
                if (File.Exists(path))
                {
                    // Refuse files held by the game before changing any other file.
                    using var source = new FileStream(path, FileMode.Open, FileAccess.ReadWrite, FileShare.Read);
                    backup = Path.Combine(directory, snapshots.Count.ToString());
                    using var destination = File.Create(backup);
                    source.CopyTo(destination);
                }
                snapshots.Add((path, backup));
            }

            try
            {
                await change();
            }
            catch (Exception original)
            {
                var errors = new List<Exception> { original };
                foreach (var (path, backup) in snapshots.AsEnumerable().Reverse())
                {
                    try
                    {
                        if (backup == null) File.Delete(path);
                        else File.Copy(backup, path, overwrite: true);
                    }
                    catch (Exception ex)
                    {
                        errors.Add(new IOException($"Could not restore '{path}': {ex.Message}", ex));
                    }
                }
                if (errors.Count > 1)
                {
                    keepBackups = true;
                    throw new AggregateException($"Mod change failed and could not be fully rolled back. Recovery copies are in '{directory}'.", errors);
                }
                throw;
            }
        }
        finally
        {
            if (!keepBackups)
            {
                try
                {
                    Directory.Delete(directory, recursive: true);
                }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
                {
                    Console.Error.WriteLine($"Warning: could not delete temporary mod rollback copies in '{directory}': {ex.Message}");
                }
            }
        }
    }
}
