namespace Tempest.CLI;

public static class TempestPathUtility
{
    /// <summary>Steam AppId used to name Proton's compat data directory (protonfixes parses digits from the path).</summary>
    public const int SteamAppId = 444090;

    public static string GetGlobalKeysDirectory() =>
        Path.GetFullPath(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "com.lowrezstudio.tempest", "keys"));

    /// <summary>Private key `mod gen-key` writes and `mod sign` uses by default; TEMPEST_SIGNING_KEY overrides it.
    /// Tempest.Scripting/scripts/package.{sh,ps1} look in the same place.</summary>
    public static string GetDefaultSigningKeyPath() =>
        Environment.GetEnvironmentVariable("TEMPEST_SIGNING_KEY") is { Length: > 0 } key
            ? Path.GetFullPath(key)
            : Path.GetFullPath(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "com.lowrezstudio.tempest", "signing", "tempest.key"));

    /// <summary>Where the embedded injector and loader are extracted to, one content-addressed subfolder per build.</summary>
    public static string GetToolsDirectory() =>
        Path.GetFullPath(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "com.lowrezstudio.tempest", "bin"));

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
