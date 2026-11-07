using System.Diagnostics;
using Tempest.CLI.Extensions;

namespace Tempest.CLI.Launcher;

internal static class GameLogs
{
    /// <summary>
    /// Prints <c>[game-logs] &lt;log folder&gt;\t&lt;temp folder&gt;</c> on stderr. The launcher reads the
    /// logs from there when the game ends; under Wine only this process knows the prefix.
    /// </summary>
    public static void Announce(Process process, string[] args, string? homedir)
    {
        var home = homedir
            ?? args.LastOrDefault(a => a.StartsWith("-homedir=", StringComparison.OrdinalIgnoreCase))?["-homedir=".Length..]
            ?? "Paladins";

        var (documents, temp) = OperatingSystem.IsWindows()
            ? (Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), Path.GetTempPath())
            : WinePrefixFolders(process);

        Console.Error.WriteLine($"[game-logs] {Path.Combine(documents, "My Games", home, "ChaosGame", "Logs")}\t{temp}");
    }

    private static (string Documents, string Temp) WinePrefixFolders(Process process)
    {
        var env = process.StartInfo.EnvironmentVariables;
        var prefix = env.ContainsKey("WINEPREFIX") && env["WINEPREFIX"] is { Length: > 0 } set ? set : WineExtensions.GetPrefix();
        var users = Path.Combine(prefix, "drive_c", "users");
        // Proton always runs as steamuser, plain Wine as the login user.
        var steamUser = Path.Combine(users, "steamuser");
        var user = Directory.Exists(steamUser) ? steamUser : Path.Combine(users, Environment.UserName);
        return (Path.Combine(user, "Documents"), Path.Combine(user, "AppData", "Local", "Temp"));
    }
}
