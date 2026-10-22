using System.Text.RegularExpressions;

namespace Tempest.CLI.Mods;

/// <summary>
/// Version requirements of <c>[mod].dependencies</c> in manifest.toml, "tempest-scripting>=1.0.0":
/// the id, then an optional requirement glued to it. The grammar mirrors the repository's
/// <c>src/lib/deps.ts</c> (and the launcher's deps.ts, the runtime's loader.rs); keep them in step:
/// comparators <c>&gt;=</c>, <c>&gt;</c>, <c>&lt;=</c>, <c>&lt;</c>, <c>=</c>/<c>==</c>, <c>^</c>, <c>~</c>,
/// a bare version meaning <c>=</c>, partial versions filled in (<c>&gt;=1.2</c> is <c>&gt;=1.2.0</c>,
/// <c>=1.2</c> is the whole 1.2 line), comparators side by side (spaces or commas) all holding,
/// <c>||</c> separating alternatives, prereleases ordered by plain precedence.
/// </summary>
public static partial class SemverRange
{
    /// <summary>The reserved dependency id for the launcher (and this CLI) version.</summary>
    public const string TempestId = "tempest";

    [GeneratedRegex("^[a-z0-9][a-z0-9._-]{1,63}$")]
    private static partial Regex ModIdPattern();

    [GeneratedRegex(@"^([A-Za-z0-9._-]+)\s*(.*)$", RegexOptions.Singleline)]
    private static partial Regex EntryPattern();

    [GeneratedRegex(@"^(>=|<=|>|<|==|=|\^|~)?\s*(.+)$")]
    private static partial Regex ComparatorPattern();

    [GeneratedRegex(@"^(\d+)(?:\.(\d+|x|\*))?(?:\.(\d+|x|\*))?(?:-([0-9A-Za-z.-]+))?$")]
    private static partial Regex PartialPattern();

    /// <summary>Splits "tempest-scripting>=1.0.0" into its id and requirement; null when malformed.</summary>
    public static ModDependency? ParseDependency(string text)
    {
        var m = EntryPattern().Match(text.Trim());
        if (!m.Success) return null;
        var id = m.Groups[1].Value.ToLowerInvariant();
        if (id != TempestId && !ModIdPattern().IsMatch(id)) return null;
        var requirement = m.Groups[2].Value.Trim();
        // What follows the id must look like a requirement, so "Tempest Core>=1" is not the id
        // "tempest" with the requirement "Core>=1".
        if (requirement.Length > 0 && "<>=^~*0123456789".IndexOf(requirement[0]) < 0) return null;
        return new ModDependency { Id = id, Requirement = requirement };
    }

    /// <summary>Whether the requirement parses; "" and "*" mean any version.</summary>
    public static bool IsValid(string requirement) => Parse(requirement) != null;

    /// <summary>Whether <paramref name="version"/> satisfies <paramref name="requirement"/>; an unparseable requirement matches nothing.</summary>
    public static bool Satisfies(string version, string requirement)
    {
        var alternatives = Parse(requirement);
        if (alternatives == null) return false;
        foreach (var alt in alternatives)
        {
            var all = true;
            foreach (var c in alt)
            {
                if (!c.Holds(version)) { all = false; break; }
            }
            if (all) return true;
        }
        return false;
    }

    private readonly record struct Comparator(string Op, string Version)
    {
        public bool Holds(string version)
        {
            var d = SemverComparer.Instance.Compare(version, Version);
            return Op switch
            {
                ">=" => d >= 0,
                ">" => d > 0,
                "<=" => d <= 0,
                "<" => d < 0,
                _ => false,
            };
        }
    }

    private static List<List<Comparator>>? Parse(string requirement)
    {
        var text = requirement.Trim();
        if (text.Length == 0 || text == "*") return [[]];
        var alternatives = new List<List<Comparator>>();
        foreach (var part in text.Split("||"))
        {
            var alt = new List<Comparator>();
            foreach (var token in part.Split([' ', '\t', ','], StringSplitOptions.RemoveEmptyEntries))
            {
                var comparators = ParseComparator(token);
                if (comparators == null) return null;
                alt.AddRange(comparators);
            }
            if (alt.Count == 0 && part.Trim().Length > 0) return null;
            alternatives.Add(alt);
        }
        return alternatives;
    }

    private static string Fmt(long[] nums, string? pre) => $"{nums[0]}.{nums[1]}.{nums[2]}{(pre == null ? "" : "-" + pre)}";

    private static string Bump(long[] nums, int at)
    {
        var o = (long[])nums.Clone();
        o[at] += 1;
        for (var i = at + 1; i < 3; i++) o[i] = 0;
        return Fmt(o, null);
    }

    private static List<Comparator>? ParseComparator(string token)
    {
        var m = ComparatorPattern().Match(token);
        if (!m.Success) return null;
        var op = m.Groups[1].Success ? m.Groups[1].Value : "=";
        var p = PartialPattern().Match(m.Groups[2].Value);
        if (!p.Success) return null;
        var major = long.Parse(p.Groups[1].Value);
        var minorRaw = p.Groups[2].Success ? p.Groups[2].Value : null;
        var patchRaw = p.Groups[3].Success ? p.Groups[3].Value : null;
        long? minor = minorRaw is null or "x" or "*" ? null : long.Parse(minorRaw);
        long? patch = minor == null || patchRaw is null or "x" or "*" ? null : long.Parse(patchRaw);
        var pre = p.Groups[4].Success ? p.Groups[4].Value : null;
        if (pre != null && patch == null) return null;
        var nums = new[] { major, minor ?? 0, patch ?? 0 };
        var low = Fmt(nums, pre);
        // The whole line a partial names: `1.2` is everything below 1.3.0, `1` everything below 2.0.0.
        var lineEnd = Bump(nums, minor == null ? 0 : 1);
        Comparator Ge(string v) => new(">=", v);
        Comparator Lt(string v) => new("<", v);
        switch (op)
        {
            case ">=": return [Ge(low)];
            case "<": return [Lt(low)];
            // `>1.2` means above the whole 1.2 line, `<=1.2` means within it.
            case ">": return patch == null ? [Ge(lineEnd)] : [new(">", low)];
            case "<=": return patch == null ? [Lt(lineEnd)] : [new("<=", low)];
            case "=":
            case "==": return patch == null ? [Ge(low), Lt(lineEnd)] : [Ge(low), new("<=", low)];
            case "~": return [Ge(low), Lt(lineEnd)];
            case "^":
                if (minor == null || major > 0) return [Ge(low), Lt(Bump(nums, 0))];
                if (patch == null || minor > 0) return [Ge(low), Lt(Bump(nums, 1))];
                return [Ge(low), Lt(Bump(nums, 2))];
            default: return null;
        }
    }
}

/// <summary>One `[mod].dependencies` entry, split. The id "tempest" is the launcher itself.</summary>
public class ModDependency
{
    public string Id { get; set; } = string.Empty;
    /// <summary>As written after the id (">=1.0.0", "^1.2"); empty means any version.</summary>
    public string Requirement { get; set; } = string.Empty;

    public override string ToString() => Id + Requirement;
}
