namespace Tempest.CLI.Mods;

public enum IniLineType
{
    Blank,
    Comment,
    Section,
    Entry
}

public class IniLine
{
    public IniLineType Type { get; set; }
    public string Raw { get; set; } = string.Empty;
    public string? SectionName { get; set; }
    public string? Key { get; set; }
    public string? Value { get; set; }
    public string? Prefix { get; set; } // "+", "-", "!", or null
}

public class IniPatcher
{
    public static List<IniLine> Parse(string filePath)
    {
        var lines = new List<IniLine>();
        if (!File.Exists(filePath)) return lines;

        string? currentSection = null;
        foreach (var rawLine in File.ReadLines(filePath))
        {
            var trimmed = rawLine.Trim();
            if (string.IsNullOrWhiteSpace(trimmed))
            {
                lines.Add(new IniLine { Type = IniLineType.Blank, Raw = rawLine });
                continue;
            }

            if (trimmed.StartsWith(';') || trimmed.StartsWith('#'))
            {
                lines.Add(new IniLine { Type = IniLineType.Comment, Raw = rawLine });
                continue;
            }

            if (trimmed.StartsWith('[') && trimmed.EndsWith(']'))
            {
                currentSection = trimmed[1..^1].Trim();
                lines.Add(new IniLine { Type = IniLineType.Section, Raw = rawLine, SectionName = currentSection });
                continue;
            }

            var eqIndex = trimmed.IndexOf('=');
            if (eqIndex > 0)
            {
                var fullKey = trimmed[..eqIndex].Trim();
                var val = trimmed[(eqIndex + 1)..].Trim();

                string? prefix = null;
                var key = fullKey;
                if (fullKey.StartsWith('+') || fullKey.StartsWith('-') || fullKey.StartsWith('!'))
                {
                    prefix = fullKey[0].ToString();
                    key = fullKey[1..].Trim();
                }

                lines.Add(new IniLine
                {
                    Type = IniLineType.Entry,
                    Raw = rawLine,
                    SectionName = currentSection,
                    Key = key,
                    Value = val,
                    Prefix = prefix
                });
            }
            else
            {
                lines.Add(new IniLine { Type = IniLineType.Blank, Raw = rawLine });
            }
        }
        return lines;
    }

    public static void Save(string filePath, List<IniLine> lines)
    {
        File.WriteAllLines(filePath, lines.Select(l => l.Raw));
    }

    public static void AddNativePackage(List<IniLine> lines, string packageName)
    {
        ApplyPatch(lines, "Engine.ScriptPackages", "+", "NativePackages", packageName);
    }

    public static void RemoveNativePackage(List<IniLine> lines, string packageName)
    {
        lines.RemoveAll(l => l.Type == IniLineType.Entry && l.Key == "NativePackages" && l.Value?.Split(';')[0].Trim() == packageName);
    }

    public static void ApplyPatch(List<IniLine> lines, string section, string prefix, string key, string value)
    {
        var sectionIndex = lines.FindIndex(l => l.Type == IniLineType.Section && string.Equals(l.SectionName, section, StringComparison.OrdinalIgnoreCase));
        if (sectionIndex == -1)
        {
            sectionIndex = lines.Count;
            lines.Add(new IniLine { Type = IniLineType.Section, Raw = $"[{section}]", SectionName = section });
        }

        var endOfSectionIndex = sectionIndex + 1;
        while (endOfSectionIndex < lines.Count && lines[endOfSectionIndex].Type != IniLineType.Section)
        {
            endOfSectionIndex++;
        }

        var insertIndex = endOfSectionIndex;
        for (var i = endOfSectionIndex - 1; i > sectionIndex; i--)
        {
            if (lines[i].Type == IniLineType.Entry)
            {
                insertIndex = i + 1;
                break;
            }
        }

        if (prefix == "+")
        {
            var exists = false;
            for (var i = sectionIndex + 1; i < endOfSectionIndex; i++)
            {
                var l = lines[i];
                if (l.Type == IniLineType.Entry && string.Equals(l.Key, key, StringComparison.OrdinalIgnoreCase) && l.Value?.Split(';')[0].Trim() == value)
                {
                    exists = true;
                    break;
                }
            }
            if (!exists)
            {
                lines.Insert(insertIndex, new IniLine
                {
                    Type = IniLineType.Entry,
                    Raw = $"+{key}={value}",
                    SectionName = section,
                    Key = key,
                    Value = value,
                    Prefix = "+"
                });
            }
        }
        else if (prefix == "-")
        {
            for (var i = sectionIndex + 1; i < endOfSectionIndex; i++)
            {
                var l = lines[i];
                if (l.Type == IniLineType.Entry && string.Equals(l.Key, key, StringComparison.OrdinalIgnoreCase) && l.Value?.Split(';')[0].Trim() == value)
                {
                    lines.RemoveAt(i);
                    i--;
                    endOfSectionIndex--;
                }
            }
        }
        else if (prefix == "!")
        {
            for (var i = sectionIndex + 1; i < endOfSectionIndex; i++)
            {
                var l = lines[i];
                if (l.Type == IniLineType.Entry && string.Equals(l.Key, key, StringComparison.OrdinalIgnoreCase))
                {
                    lines.RemoveAt(i);
                    i--;
                    endOfSectionIndex--;
                }
            }
        }
        else
        {
            var existingIndex = -1;
            for (var i = sectionIndex + 1; i < endOfSectionIndex; i++)
            {
                var l = lines[i];
                if (l.Type == IniLineType.Entry && string.Equals(l.Key, key, StringComparison.OrdinalIgnoreCase))
                {
                    existingIndex = i;
                    break;
                }
            }

            if (existingIndex != -1)
            {
                lines[existingIndex] = new IniLine
                {
                    Type = IniLineType.Entry,
                    Raw = $"{key}={value}",
                    SectionName = section,
                    Key = key,
                    Value = value,
                    Prefix = null
                };
            }
            else
            {
                lines.Insert(insertIndex, new IniLine
                {
                    Type = IniLineType.Entry,
                    Raw = $"{key}={value}",
                    SectionName = section,
                    Key = key,
                    Value = value,
                    Prefix = null
                });
            }
        }
    }
}
