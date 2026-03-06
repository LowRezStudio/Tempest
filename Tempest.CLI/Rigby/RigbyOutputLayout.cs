using Tempest.CLI.Rigby.Models;

namespace Tempest.CLI.Rigby;

internal static class RigbyOutputLayout
{
    public static string NormalizePrefix(string? prefix)
    {
        if (string.IsNullOrWhiteSpace(prefix))
            return string.Empty;

        return prefix.Trim().Trim('/').Trim('\\');
    }

    public static string ResolveEffectivePrefix(string outRoot, string manifestPrefix, IReadOnlyList<RigbyFile> files)
    {
        if (string.IsNullOrWhiteSpace(manifestPrefix))
            return string.Empty;

        if (ShouldApplyManifestPrefix(outRoot, manifestPrefix))
            return manifestPrefix;

        var sampleCount = Math.Min(files.Count, 256);
        var unprefixedExisting = 0;
        var prefixedExisting = 0;

        for (var i = 0; i < sampleCount; i++)
        {
            var file = files[i];
            if (File.Exists(Path.Combine(outRoot, file.Path)))
                unprefixedExisting += 1;

            if (File.Exists(Path.Combine(outRoot, manifestPrefix, file.Path)))
                prefixedExisting += 1;
        }

        return prefixedExisting > unprefixedExisting ? manifestPrefix : string.Empty;
    }

    public static bool IsPathWithinRoot(string rootPath, string candidatePath)
    {
        var relative = Path.GetRelativePath(rootPath, candidatePath);
        return relative != ".."
            && !relative.StartsWith($"..{Path.DirectorySeparatorChar}", StringComparison.Ordinal)
            && !relative.StartsWith($"..{Path.AltDirectorySeparatorChar}", StringComparison.Ordinal)
            && !Path.IsPathRooted(relative);
    }

    public static int DeleteUnexpectedFiles(string outRoot, HashSet<string> expectedFiles)
    {
        if (!Directory.Exists(outRoot))
            return 0;

        var deletedFiles = 0;
        foreach (var path in Directory.EnumerateFiles(outRoot, "*", SearchOption.AllDirectories))
        {
            var fullPath = Path.GetFullPath(path);
            if (expectedFiles.Contains(fullPath))
                continue;

            File.Delete(fullPath);
            deletedFiles += 1;
        }

        foreach (var dir in Directory.EnumerateDirectories(outRoot, "*", SearchOption.AllDirectories).OrderByDescending(static x => x.Length))
        {
            if (!Directory.EnumerateFileSystemEntries(dir).Any())
                Directory.Delete(dir);
        }

        return deletedFiles;
    }

    public static void EnsureParent(string path)
    {
        var parent = Path.GetDirectoryName(path);
        if (!string.IsNullOrWhiteSpace(parent))
            Directory.CreateDirectory(parent);
    }

    public static StringComparer GetPathComparer()
        => OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;

    private static bool ShouldApplyManifestPrefix(string outRoot, string manifestPrefix)
    {
        if (string.IsNullOrWhiteSpace(manifestPrefix))
            return false;

        var normalizedOutRoot = outRoot.Replace('\\', '/').TrimEnd('/');
        var normalizedPrefix = manifestPrefix.Replace('\\', '/').Trim('/');
        var comparison = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;

        if (string.Equals(normalizedOutRoot, normalizedPrefix, comparison))
            return false;

        return !normalizedOutRoot.EndsWith("/" + normalizedPrefix, comparison);
    }
}
