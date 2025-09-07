namespace Tempest.CLI.Launcher;

public static class LauncherUtility
{
    public static string GetExecutablePath(string path, string? overridePlatform)
    {
        var exePath = Path.GetFullPath(path);
        
        if (!Path.GetExtension(exePath).Equals(".exe", StringComparison.OrdinalIgnoreCase))
        {
            var gameFolder = new DirectoryInfo(exePath);

            while (gameFolder != null)
            {
                var fullName = gameFolder.FullName;

                if (Directory.Exists(Path.Join(fullName, "Binaries")) &&
                    Directory.Exists(Path.Join(fullName, "Engine")))
                {
                    break;
                }

                gameFolder = gameFolder.Parent;
            }

            if (gameFolder == null)
            {
                throw new Exception("Couldn't find the Paladins game folder (containing Binaries and Engine folders)");
            }

            exePath = Path.Join(gameFolder.FullName, "Binaries", overridePlatform ?? "Win64", "Paladins.exe");

            if (overridePlatform == "Win32" || !Environment.Is64BitProcess || !File.Exists(exePath))
            {
                exePath = Path.Join(gameFolder.FullName, "Binaries", "Win32", "Paladins.exe");
            }

            if (!File.Exists(exePath))
            {
                throw new Exception("Couldn't find the Paladins game folder (containing Binaries and Engine folders)");
            }
        }

        return exePath;
    }
}