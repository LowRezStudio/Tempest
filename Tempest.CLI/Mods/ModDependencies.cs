using System.Reflection;

namespace Tempest.CLI.Mods;

/// <summary>
/// What installed mods need from each other and from the launcher. Dependencies come from the
/// manifest and are kept on the <see cref="ModRecord"/>, so nothing here reads the mod folders.
/// </summary>
public static class ModDependencies
{
    private static string? _tempestVersion;
    private static bool _tempestVersionRead;

    /// <summary>
    /// The Tempest version this CLI belongs to: the launcher's, stamped at build time
    /// (<c>-p:TempestVersion=</c>, see Tempest.Launcher/scripts/build-cli.js), overridden by the
    /// <c>TEMPEST_VERSION</c> environment variable. Null for an unstamped developer build, which
    /// then never refuses a mod for its <c>tempest</c> requirement.
    /// </summary>
    public static string? TempestVersion
    {
        get
        {
            if (_tempestVersionRead) return _tempestVersion;
            _tempestVersionRead = true;
            var env = Environment.GetEnvironmentVariable("TEMPEST_VERSION");
            if (!string.IsNullOrWhiteSpace(env)) return _tempestVersion = env.Trim();
            var stamped = typeof(ModDependencies).Assembly
                .GetCustomAttributes<AssemblyMetadataAttribute>()
                .FirstOrDefault(a => a.Key == "TempestVersion")?.Value;
            return _tempestVersion = string.IsNullOrWhiteSpace(stamped) ? null : stamped.Trim();
        }
    }

    /// <summary>
    /// The <c>tempest</c> requirement of <paramref name="dependencies"/> that <paramref name="tempestVersion"/>
    /// does not meet, or null when it does (or when the version is unknown).
    /// </summary>
    public static string? UnmetTempestRequirement(IEnumerable<ModDependency> dependencies, string? tempestVersion)
    {
        if (tempestVersion == null) return null;
        var req = dependencies.FirstOrDefault(d => d.Id == SemverRange.TempestId);
        if (req == null) return null;
        return SemverRange.Satisfies(tempestVersion, req.Requirement) ? null : req.Requirement;
    }

    /// <summary>
    /// The mod dependencies of <paramref name="dependencies"/> that no enabled installed mod
    /// satisfies (missing, disabled, or the wrong version). The <c>tempest</c> entry is not a mod.
    /// </summary>
    public static List<ModDependency> Missing(IEnumerable<ModDependency> dependencies, IEnumerable<ModRecord> installed)
    {
        var byId = installed
            .Where(m => m.Enabled)
            .GroupBy(m => m.Id, StringComparer.OrdinalIgnoreCase)
            .ToDictionary(g => g.Key, g => g.First(), StringComparer.OrdinalIgnoreCase);
        var missing = new List<ModDependency>();
        foreach (var dep in dependencies)
        {
            if (dep.Id == SemverRange.TempestId) continue;
            if (byId.TryGetValue(dep.Id, out var mod) && SemverRange.Satisfies(mod.Version, dep.Requirement)) continue;
            missing.Add(dep);
        }
        return missing;
    }

    /// <summary>Enabled installed mods that depend on <paramref name="modId"/>.</summary>
    public static List<ModRecord> Dependants(string modId, IEnumerable<ModRecord> installed) =>
        installed
            .Where(m => m.Enabled && !string.Equals(m.Id, modId, StringComparison.OrdinalIgnoreCase)
                        && m.Dependencies.Any(d => string.Equals(d.Id, modId, StringComparison.OrdinalIgnoreCase)))
            .ToList();

    /// <summary>
    /// At launch: says on stderr which enabled mods miss a dependency (or need a newer Tempest).
    /// Nothing is disabled; a Lua mod without its runtime simply does not run, and the launcher
    /// shows the same information in its list.
    /// </summary>
    public static void WarnAtLaunch(string gamePath)
    {
        try
        {
            if (!File.Exists(ModCommands.GetMetadataPath(gamePath))) return;
            var mods = ModCommands.LoadMetadata(gamePath);
            foreach (var mod in mods.Where(m => m.Enabled && m.Dependencies.Count > 0))
            {
                var missing = Missing(mod.Dependencies, mods);
                if (missing.Count > 0)
                    Console.Error.WriteLine($"Warning: '{mod.Name}' needs {string.Join(", ", missing)}, which is not installed and enabled.");
                var tempest = UnmetTempestRequirement(mod.Dependencies, TempestVersion);
                if (tempest != null)
                    Console.Error.WriteLine($"Warning: '{mod.Name}' needs Tempest {tempest}; this is {TempestVersion}.");
            }
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Warning: could not check mod dependencies: {ex.Message}");
        }
    }
}
