using System.Text.RegularExpressions;

namespace Tempest.CLI.Rigby;

internal static class RigbyManifestInputResolver
{
    public static IReadOnlyList<string> ExpandManifestInputs(IEnumerable<string> patterns)
    {
        var results = new List<string>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var cwd = Directory.GetCurrentDirectory();

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

            foreach (var file in Directory.EnumerateFiles(cwd, "*", SearchOption.AllDirectories))
            {
                var relative = Path.GetRelativePath(cwd, file).Replace('\\', '/');
                if (!GlobMatch(relative, glob))
                    continue;

                var fullPath = Path.GetFullPath(file);
                if (seen.Add(fullPath))
                    results.Add(fullPath);
            }
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

    private static bool GlobMatch(string input, string pattern)
    {
        var regex = "^" + Regex.Escape(pattern)
            .Replace("\\*\\*", "<<double-star>>")
            .Replace("\\*", "[^/]*")
            .Replace("<<double-star>>", ".*")
            .Replace("\\?", "[^/]")
            + "$";

        return Regex.IsMatch(input, regex, RegexOptions.CultureInvariant);
    }
}
