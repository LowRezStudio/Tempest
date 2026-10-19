namespace Tempest.CLI.Mods;

public class ModRecord
{
    public string Id { get; set; } = string.Empty;
    public string Name { get; set; } = string.Empty;
    public string Author { get; set; } = "Unknown";
    public List<ModAuthor> Authors { get; set; } = [];
    public string Version { get; set; } = "1.0.0";
    public bool Enabled { get; set; } = true;
    public string Kind { get; set; } = string.Empty;
    public string OriginalPath { get; set; } = string.Empty;
    public List<string> InstalledFiles { get; set; } = [];
    public List<string> OwnedFiles { get; set; } = [];
    public int MetadataVersion { get; set; } = 1;
    public string Readme { get; set; } = string.Empty;
    public string ReadmeContent { get; set; } = string.Empty;
    /// <summary>Fingerprint of the key that signed the package (trusted, or untrusted and accepted); empty when unsigned.</summary>
    public string Signer { get; set; } = string.Empty;
}

public class ModConflictInfo
{
    public string ModId { get; set; } = string.Empty;
    public string ModName { get; set; } = string.Empty;
    public string ModVersion { get; set; } = string.Empty;
    public List<string> ConflictingFiles { get; set; } = [];
    public bool IsSameMod { get; set; } = false;
}

public class ModInstallResult
{
    public bool Success { get; set; }
    public string Message { get; set; } = string.Empty;
    public ModRecord? Mod { get; set; }
    public bool Conflict { get; set; }
    public bool IsModConflict { get; set; }
    /// <summary>Code signed by a key that is not trusted: install again with allowUntrusted once the user agrees.</summary>
    public bool Unverified { get; set; }
    /// <summary>Who signed an <see cref="Unverified"/> mod, "user id [fingerprint]".</summary>
    public string? Signer { get; set; }
    /// <summary>The published key lists could not be reached, so only the built-in keys were trusted.</summary>
    public bool KeyListOffline { get; set; }
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
