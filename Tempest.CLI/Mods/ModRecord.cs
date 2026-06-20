using System.Collections.Generic;

namespace Tempest.CLI.Mods;

public class ModRecord
{
    public string Id { get; set; } = string.Empty;
    public string Name { get; set; } = string.Empty;
    public string Author { get; set; } = "Unknown";
    public string Version { get; set; } = "1.0.0";
    public bool Enabled { get; set; } = true;
    public string Kind { get; set; } = string.Empty; // "Voice", "Asset", "NativePackage"
    public string OriginalPath { get; set; } = string.Empty;
    public List<string> InstalledFiles { get; set; } = [];
}

public class ModInstallResult
{
    public bool Success { get; set; }
    public string Message { get; set; } = string.Empty;
    public ModRecord? Mod { get; set; }
    public bool Conflict { get; set; }
    public bool IsModConflict { get; set; }
}

public class ModListResult
{
    public List<ModRecord> Mods { get; set; } = [];
}

public class ModBulkResult
{
    public List<ModInstallResult> Results { get; set; } = [];
}
