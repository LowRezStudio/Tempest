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
    /// <summary>The mod's files matched its signed CHECKSUMS at the last launch (also set at install time).</summary>
    public bool Verified { get; set; }
    /// <summary>SHA-256 of the mod folder's CHECKSUMS content when it was last verified; pins it against being swapped out.</summary>
    public string ManifestHash { get; set; } = string.Empty;
    /// <summary>`[mod].dependencies` of the manifest at install time; "tempest" entries are the launcher version requirement.</summary>
    public List<ModDependency> Dependencies { get; set; } = [];
    /// <summary>When the mod was first installed (UTC). Updates and reinstalls keep it, so the list stays in the order mods were added.</summary>
    public DateTime? InstalledAt { get; set; }
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
    /// <summary>The mod was installed, but these dependencies are not installed and enabled (or not at a version that fits); the launcher offers to install them.</summary>
    public List<ModDependency>? MissingDependencies { get; set; }
    /// <summary>The install was refused because the package needs this Tempest version (its "tempest" dependency) and the running one is older.</summary>
    public string? TempestRequired { get; set; }
    /// <summary>Repository dependencies installed along with the mod (`mod install &lt;id&gt;`), as "id@version".</summary>
    public List<string>? InstalledDependencies { get; set; }
    /// <summary>Refused before anything was written: files the install would overwrite or delete are held open by another process (usually the running game). The launcher queues the install and retries once the game has closed.</summary>
    public bool FilesInUse { get; set; }
    /// <summary>The files of a <see cref="FilesInUse"/> refusal, relative to the game folder.</summary>
    public List<string>? LockedFiles { get; set; }
    /// <summary>`mod install --defer`: every check passed (trust, conflicts, checksums, files in use), but nothing was written. The launcher queues it while the game runs.</summary>
    public bool Deferred { get; set; }
}

public class ModListResult
{
    public List<ModRecord> Mods { get; set; } = [];
}

public class ModBulkResult
{
    public List<ModInstallResult> Results { get; set; } = [];
}
