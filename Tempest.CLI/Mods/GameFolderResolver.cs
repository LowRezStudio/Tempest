namespace Tempest.CLI.Mods;

public static class GameFolderResolver
{
    public static string Resolve(string startingPath)
    {
        var current = startingPath;
        if (File.Exists(startingPath))
        {
            current = Path.GetDirectoryName(startingPath) ?? startingPath;
        }

        while (true)
        {
            if (HasRequiredGameDirs(current))
            {
                return current;
            }

            var parent = Path.GetDirectoryName(current);
            if (parent == null || parent == current)
            {
                break;
            }
            current = parent;
        }

        throw new DirectoryNotFoundException($"Could not find game folder containing 'Binaries' and 'Engine' starting from: {startingPath}");
    }

    private static bool HasRequiredGameDirs(string dirPath)
    {
        try
        {
            if (!Directory.Exists(dirPath)) return false;
            var binaries = Path.Combine(dirPath, "Binaries");
            var engine = Path.Combine(dirPath, "Engine");
            return Directory.Exists(binaries) && Directory.Exists(engine);
        }
        catch
        {
            return false;
        }
    }
}
