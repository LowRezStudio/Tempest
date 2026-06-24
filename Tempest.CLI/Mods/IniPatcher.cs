namespace Tempest.CLI.Mods;

public enum IniLineType
{
    Blank,
    Comment,
    Section,
    Entry
}

public sealed record IniLine
{
    public required IniLineType Type { get; init; }
    public required string Raw { get; init; }
    public string? SectionName { get; init; }
    public string? Key { get; init; }
    public string? Value { get; init; }
    public string? Prefix { get; init; } // "+", "-", "!", or null
}

public static class IniPatcher
{
    public static List<IniLine> Parse(string filePath)
    {
        if (!File.Exists(filePath)) return [];

        var lines = new List<IniLine>();
        string? currentSection = null;

        foreach (var rawLine in File.ReadLines(filePath))
        {
            lines.Add(ParseLine(rawLine, ref currentSection));
        }

        return lines;
    }

    private static IniLine ParseLine(string rawLine, ref string? currentSection)
    {
        var trimmed = rawLine.Trim();

        if (string.IsNullOrWhiteSpace(trimmed))
            return new IniLine { Type = IniLineType.Blank, Raw = rawLine };

        if (trimmed[0] is ';' or '#')
            return new IniLine { Type = IniLineType.Comment, Raw = rawLine };

        if (trimmed is ['[', .., ']'])
        {
            currentSection = trimmed[1..^1].Trim();
            return new IniLine { Type = IniLineType.Section, Raw = rawLine, SectionName = currentSection };
        }

        var eqIndex = trimmed.IndexOf('=');
        if (eqIndex > 0)
        {
            var fullKey = trimmed[..eqIndex].Trim();
            var val = trimmed[(eqIndex + 1)..].Trim();

            var (prefix, key) = fullKey[0] is '+' or '-' or '!'
                ? (fullKey[0].ToString(), fullKey[1..].Trim())
                : (null, fullKey);

            return new IniLine
            {
                Type = IniLineType.Entry,
                Raw = rawLine,
                SectionName = currentSection,
                Key = key,
                Value = val,
                Prefix = prefix
            };
        }

        return new IniLine { Type = IniLineType.Blank, Raw = rawLine };
    }

    public static void Save(string filePath, List<IniLine> lines)
        => File.WriteAllLines(filePath, lines.Select(l => l.Raw));

    public static void AddNativePackage(List<IniLine> lines, string packageName)
        => ApplyPatch(lines, "Engine.ScriptPackages", "+", "NativePackages", packageName);

    public static void RemoveNativePackage(List<IniLine> lines, string packageName)
    {
        lines.RemoveAll(l =>
            l.Type == IniLineType.Entry &&
            l.Key == "NativePackages" &&
            ValuesEqual(l.Value, packageName));
    }

    public static IniLine CreateEntry(string section, string key, string value, string? prefix)
    {
        var raw = prefix is null ? $"{key}={value}" : $"{prefix}{key}={value}";
        return new IniLine
        {
            Type = IniLineType.Entry,
            Raw = raw,
            SectionName = section,
            Key = key,
            Value = value,
            Prefix = prefix
        };
    }

    public static void ApplyPatch(List<IniLine> lines, string section, string prefix, string key, string value)
    {
        var (sectionStart, sectionEnd) = GetOrCreateSection(lines, section);
        var insertIndex = FindInsertIndex(lines, sectionStart, sectionEnd);

        switch (prefix)
        {
            case "+":
                if (!HasMatchingEntry(lines, sectionStart, sectionEnd, key, value))
                    lines.Insert(insertIndex, CreateEntry(section, key, value, "+"));
                break;
            case "-":
                RemoveMatchingEntries(lines, sectionStart, sectionEnd, key, value);
                break;
            case "!":
                RemoveAllKeys(lines, sectionStart, sectionEnd, key);
                break;
            default:
                var existingIndex = FindKeyIndex(lines, sectionStart, sectionEnd, key);
                if (existingIndex >= 0)
                {
                    var existing = lines[existingIndex];
                    if (existing.Prefix is null && ValuesEqual(existing.Value, value))
                        return;
                    lines[existingIndex] = CreateEntry(section, key, value, null);
                }
                else
                {
                    lines.Insert(insertIndex, CreateEntry(section, key, value, null));
                }
                break;
        }
    }

    public static int FindKeyIndex(List<IniLine> lines, string section, string key)
    {
        var (start, end) = GetSectionRange(lines, section);
        return start < 0 ? -1 : FindKeyIndex(lines, start, end, key);
    }

    public static (int Start, int End) GetSectionRange(List<IniLine> lines, string section)
    {
        var start = lines.FindIndex(l =>
            l.Type == IniLineType.Section &&
            string.Equals(l.SectionName, section, StringComparison.OrdinalIgnoreCase));

        if (start < 0) return (-1, -1);

        var end = lines.FindIndex(start + 1, l => l.Type == IniLineType.Section);
        return end < 0 ? (start, lines.Count) : (start, end);
    }

    public static void RemoveAllInSection(List<IniLine> lines, string section, Predicate<IniLine> predicate)
    {
        var (start, end) = GetSectionRange(lines, section);
        if (start < 0) return;

        for (var i = start + 1; i < end; i++)
        {
            if (!predicate(lines[i])) continue;

            lines.RemoveAt(i);
            i--;
            end--;
        }
    }

    private static (int Start, int End) GetOrCreateSection(List<IniLine> lines, string section)
    {
        var range = GetSectionRange(lines, section);
        if (range.Start >= 0) return range;

        lines.Add(new IniLine { Type = IniLineType.Section, Raw = $"[{section}]", SectionName = section });
        return (lines.Count - 1, lines.Count);
    }

    private static int FindInsertIndex(List<IniLine> lines, int sectionStart, int sectionEnd)
    {
        var count = sectionEnd - sectionStart - 1;
        if (count <= 0) return sectionStart + 1;

        var lastEntry = lines.FindLastIndex(sectionStart + 1, count, l => l.Type == IniLineType.Entry);
        return lastEntry >= 0 ? lastEntry + 1 : sectionStart + 1;
    }

    private static int FindKeyIndex(List<IniLine> lines, int sectionStart, int sectionEnd, string key)
    {
        for (var i = sectionStart + 1; i < sectionEnd; i++)
        {
            var l = lines[i];
            if (l.Type == IniLineType.Entry &&
                string.Equals(l.Key, key, StringComparison.OrdinalIgnoreCase))
                return i;
        }
        return -1;
    }

    private static bool HasMatchingEntry(List<IniLine> lines, int sectionStart, int sectionEnd, string key, string value)
    {
        for (var i = sectionStart + 1; i < sectionEnd; i++)
        {
            var l = lines[i];
            if (l.Type == IniLineType.Entry &&
                string.Equals(l.Key, key, StringComparison.OrdinalIgnoreCase) &&
                ValuesEqual(l.Value, value))
                return true;
        }
        return false;
    }

    private static void RemoveMatchingEntries(List<IniLine> lines, int sectionStart, int sectionEnd, string key, string value)
    {
        for (var i = sectionStart + 1; i < sectionEnd; i++)
        {
            var l = lines[i];
            if (l.Type == IniLineType.Entry &&
                string.Equals(l.Key, key, StringComparison.OrdinalIgnoreCase) &&
                ValuesEqual(l.Value, value))
            {
                lines.RemoveAt(i);
                i--;
                sectionEnd--;
            }
        }
    }

    private static void RemoveAllKeys(List<IniLine> lines, int sectionStart, int sectionEnd, string key)
    {
        for (var i = sectionStart + 1; i < sectionEnd; i++)
        {
            var l = lines[i];
            if (l.Type == IniLineType.Entry &&
                string.Equals(l.Key, key, StringComparison.OrdinalIgnoreCase))
            {
                lines.RemoveAt(i);
                i--;
                sectionEnd--;
            }
        }
    }

    private static bool ValuesEqual(string? a, string? b) =>
        string.Equals(GetFirstValue(a), GetFirstValue(b), StringComparison.Ordinal);

    private static string? GetFirstValue(string? value)
    {
        if (string.IsNullOrWhiteSpace(value)) return value;
        var semicolonIndex = value.IndexOf(';');
        return semicolonIndex < 0 ? value.Trim() : value[..semicolonIndex].Trim();
    }
}
