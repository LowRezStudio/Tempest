namespace Tempest.CLI.Mods;

/// <summary>
/// Game-relative paths as the game folder spells them. Mods may name a game file in any case
/// (<c>chaosgame/cookedpcconsole/maeve_assets_sf.upk</c>); on a case-sensitive file system
/// (Linux) that would create a second file next to the game's <c>MAEVE_ASSETS_SF.upk</c>
/// instead of replacing it, so every part that exists under the game folder takes the casing
/// on disk. Parts that do not exist yet keep the mod's spelling.
/// </summary>
public static class GamePath
{
    /// <summary>The relative path with each existing component in its on-disk casing.</summary>
    public static string Canonical(string root, string relativePath)
    {
        var parts = relativePath.Replace('\\', '/').Split('/', StringSplitOptions.RemoveEmptyEntries);
        var dir = root;
        var outParts = new List<string>(parts.Length);
        var exists = Directory.Exists(dir);
        foreach (var part in parts)
        {
            var name = part;
            if (exists && part is not "." and not "..")
            {
                var exact = Path.Combine(dir, part);
                if (!File.Exists(exact) && !Directory.Exists(exact))
                {
                    // Only a case-sensitive file system gets here with a differently cased name.
                    name = Directory.EnumerateFileSystemEntries(dir)
                        .Select(Path.GetFileName)
                        .FirstOrDefault(n => string.Equals(n, part, StringComparison.OrdinalIgnoreCase)) ?? part;
                }
            }
            outParts.Add(name);
            dir = Path.Combine(dir, name);
            exists = exists && Directory.Exists(dir);
        }
        return string.Join('/', outParts);
    }

    /// <summary>
    /// The full path of <paramref name="relativePath"/> under <paramref name="root"/>, with every
    /// part that exists taking its on-disk casing. Used for files a mod folder stores under the
    /// spelling it shipped (an older install, or a package made on a case-insensitive disk).
    /// </summary>
    public static string Resolve(string root, string relativePath) =>
        Path.IsPathRooted(relativePath)
            ? Path.GetFullPath(relativePath)
            : Path.Combine(root, Canonical(root, relativePath));

    /// <summary>
    /// A package entry with its game path in the game folder's casing: <c>files/&lt;game path&gt;</c>
    /// and <c>deltas/&lt;game path&gt;.tdelta</c>; other entries are returned unchanged.
    /// </summary>
    public static string CanonicalEntry(string root, string entryPath)
    {
        if (entryPath.StartsWith("files/", StringComparison.OrdinalIgnoreCase))
            return "files/" + Canonical(root, entryPath["files/".Length..]);
        if (DeltaResolver.TargetOf(entryPath) is { } target)
            return "deltas/" + Canonical(root, target) + entryPath[^".tdelta".Length..];
        return entryPath;
    }
}
