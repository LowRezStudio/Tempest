namespace Tempest.CLI.Rigby;

internal static class RigbyManifestInputResolver
{
    public static IReadOnlyList<string> ExpandManifestInputs(IEnumerable<string> patterns)
    {
        var results = new List<string>();
        var seen = new HashSet<string>(RigbyOutputLayout.GetPathComparer());
        var cwd = Directory.GetCurrentDirectory();
        List<string>? globs = null;

        foreach (var raw in patterns)
        {
            if (string.IsNullOrWhiteSpace(raw))
                continue;

            if (IsHttpUrl(raw, out var manifestUri))
            {
                var url = manifestUri.AbsoluteUri;
                if (seen.Add(url))
                    results.Add(url);

                continue;
            }

            if (File.Exists(raw))
            {
                var fullPath = Path.GetFullPath(raw);
                if (seen.Add(fullPath))
                    results.Add(fullPath);

                continue;
            }

            var glob = raw.Replace('\\', '/');
            if (!glob.Contains('*') && !glob.Contains('?'))
                continue;

            (globs ??= []).Add(glob);
        }

        if (globs is null)
            return results;

        // One walk of the tree for every glob, instead of a walk and a compiled regex per file.
        var options = new EnumerationOptions { RecurseSubdirectories = true, IgnoreInaccessible = true };
        foreach (var file in Directory.EnumerateFiles(cwd, "*", options))
        {
            var relative = Path.GetRelativePath(cwd, file).Replace('\\', '/');
            if (!globs.Any(glob => GlobMatches(relative, glob)))
                continue;

            var fullPath = Path.GetFullPath(file);
            if (seen.Add(fullPath))
                results.Add(fullPath);
        }

        return results;
    }

    private static bool IsHttpUrl(string value, out Uri uri)
    {
        if (Uri.TryCreate(value, UriKind.Absolute, out uri!)
            && (uri.Scheme == Uri.UriSchemeHttp || uri.Scheme == Uri.UriSchemeHttps))
            return true;

        uri = null!;
        return false;
    }

    /// <summary>
    /// Wildcard match over the whole relative path: <c>*</c> runs inside one directory, <c>?</c> is
    /// one character inside one directory, and <c>**</c> crosses directories. Same rules the regex
    /// the old implementation built per file encoded, without the regex.
    /// </summary>
    private static bool GlobMatches(ReadOnlySpan<char> path, ReadOnlySpan<char> pattern)
    {
        // previous[j]: path[..i] matches pattern[..j]; starts at i == 0.
        Span<bool> previous = pattern.Length <= 256 ? stackalloc bool[pattern.Length + 1] : new bool[pattern.Length + 1];
        Span<bool> current = pattern.Length <= 256 ? stackalloc bool[pattern.Length + 1] : new bool[pattern.Length + 1];

        previous[0] = true;
        for (var j = 1; j <= pattern.Length; j++)
            previous[j] = previous[j - 1] && pattern[j - 1] == '*';

        for (var i = 1; i <= path.Length; i++)
        {
            current[0] = false;
            for (var j = 1; j <= pattern.Length; j++)
            {
                var symbol = pattern[j - 1];
                if (symbol == '*')
                {
                    var crossesDirectories = j >= 2 && pattern[j - 2] == '*';
                    current[j] = current[j - 1] || (previous[j] && (crossesDirectories || path[i - 1] != '/'));
                }
                else
                {
                    var matches = symbol == '?' ? path[i - 1] != '/' : symbol == path[i - 1];
                    current[j] = matches && previous[j - 1];
                }
            }

            var swap = previous;
            previous = current;
            current = swap;
        }

        return previous[pattern.Length];
    }
}
