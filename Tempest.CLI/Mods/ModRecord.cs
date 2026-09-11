namespace Tempest.CLI.Mods;

public class ModRecord
{
    public string Id { get; set; } = string.Empty;
    public string Name { get; set; } = string.Empty;
    public string Author { get; set; } = "Unknown";
    public List<ModAuthor> Authors { get; set; } = [];
    public string Version { get; set; } = "1.0.0";
    public bool Enabled { get; set; } = true;
    public string Kind { get; set; } = string.Empty; // "Voice", "Asset", "NativePackage"
    public string OriginalPath { get; set; } = string.Empty;
    public List<string> InstalledFiles { get; set; } = [];
    public string Readme { get; set; } = string.Empty;
    public string ReadmeContent { get; set; } = string.Empty;
}

public class ModConflictInfo
{
    public string ModId { get; set; } = string.Empty;
    public string ModName { get; set; } = string.Empty;
    public string ModVersion { get; set; } = string.Empty;
    public List<string> ConflictingFiles { get; set; } = [];
}

public class ModInstallResult
{
    public bool Success { get; set; }
    public string Message { get; set; } = string.Empty;
    public ModRecord? Mod { get; set; }
    public bool Conflict { get; set; }
    public bool IsModConflict { get; set; }
    public bool Unverified { get; set; }
    public List<ModConflictInfo>? ConflictingMods { get; set; }
    public string? NewModName { get; set; }
}

public class ModListResult
{
    public List<ModRecord> Mods { get; set; } = [];
}

public class ModBulkResult
{
    public List<ModInstallResult> Results { get; set; } = [];
}
