namespace Tempest.CLI;

public static class TempestPathUtility
{
    /// <summary>Steam AppId used to name Proton's compat data directory (protonfixes parses digits from the path).</summary>
    public const int SteamAppId = 444090;

    public static string GetGlobalKeysDirectory() =>
        Path.GetFullPath(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "com.lowrezstudio.tempest", "keys"));

    public static string GetWinePrefixDirectory() =>
        Path.GetFullPath(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "com.lowrezstudio.tempest", "prefix"));

    /// <summary>STEAM_COMPAT_DATA_PATH for Proton launches; keeps Proton prefixes separate from system Wine ones.
    /// Ends with the Steam AppId because Proton's protonfixes extracts a game id from the path's digits.</summary>
    public static string GetProtonDataDirectory() =>
        Path.GetFullPath(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "com.lowrezstudio.tempest", $"prefix-proton-{SteamAppId}"));

    public static string GetLocalKeysDirectory(string resolvedGame) =>
        Path.Combine(resolvedGame, ".tempest", "keys");

    public static string GetLocalModsDirectory(string resolvedGame) =>
        Path.Combine(resolvedGame, ".tempest", "mods");

    public static string GetLocalV2ModDirectory(string resolvedGame, string modId) =>
        Path.Combine(resolvedGame, ".tempest", "v2", "mods", modId);

    public static string GetLocalV2BackupPath(string resolvedGame, string relativePath) =>
        Path.Combine(resolvedGame, ".tempest", "v2", "backup", relativePath);

    public static string GetLocalV2BackupDirectory(string resolvedGame, string modId) =>
        Path.Combine(resolvedGame, ".tempest", "v2", "backup", modId);

    public static string GetLocalV2IniBackupPath(string resolvedGame, string modId, string relativePath) =>
        Path.Combine(resolvedGame, ".tempest", "v2", "ini-backup", modId, relativePath);

    public static string GetLocalV2IniBackupDirectory(string resolvedGame, string modId) =>
        Path.Combine(resolvedGame, ".tempest", "v2", "ini-backup", modId);

    public static string GetLocalV1ModDirectory(string resolvedGame, string modId) =>
        Path.Combine(resolvedGame, ".tempest", "v1", "mods", modId);

    public static string GetLocalV1BackupDirectory(string resolvedGame) =>
        Path.Combine(resolvedGame, ".tempest", "v1", "backup");

    public static string GetLocalV1BackupPath(string resolvedGame, string fileName) =>
        Path.Combine(resolvedGame, ".tempest", "v1", "backup", fileName);
}
