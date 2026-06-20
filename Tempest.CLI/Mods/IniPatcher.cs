using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;

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
                string key = fullKey;
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
        var targetLine = $"+NativePackages={packageName} ; tempest";

        if (lines.Any(l => l.Type == IniLineType.Entry && l.Key == "NativePackages" && l.Value?.Split(';')[0].Trim() == packageName))
        {
            return;
        }

        var index = lines.FindIndex(l => l.Type == IniLineType.Entry && l.Key == "NativePackages" && l.Value?.Split(';')[0].Trim() == "BattleClient");
        if (index != -1)
        {
            var sectionName = lines[index].SectionName;
            lines.Insert(index + 1, new IniLine
            {
                Type = IniLineType.Entry,
                Raw = targetLine,
                SectionName = sectionName,
                Key = "NativePackages",
                Value = packageName + " ; tempest",
                Prefix = "+"
            });
        }
        else
        {
            var sectionIndex = lines.FindIndex(l => l.Type == IniLineType.Section && l.SectionName == "Engine.PackagesToAlwaysCook");
            if (sectionIndex != -1)
            {
                int insertIndex = sectionIndex + 1;
                while (insertIndex < lines.Count && lines[insertIndex].Type != IniLineType.Section)
                {
                    insertIndex++;
                }
                lines.Insert(insertIndex, new IniLine
                {
                    Type = IniLineType.Entry,
                    Raw = targetLine,
                    SectionName = "Engine.PackagesToAlwaysCook",
                    Key = "NativePackages",
                    Value = packageName + " ; tempest",
                    Prefix = "+"
                });
            }
            else
            {
                lines.Add(new IniLine { Type = IniLineType.Section, Raw = "[Engine.PackagesToAlwaysCook]", SectionName = "Engine.PackagesToAlwaysCook" });
                lines.Add(new IniLine
                {
                    Type = IniLineType.Entry,
                    Raw = targetLine,
                    SectionName = "Engine.PackagesToAlwaysCook",
                    Key = "NativePackages",
                    Value = packageName + " ; tempest",
                    Prefix = "+"
                });
            }
        }
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

        int endOfSectionIndex = sectionIndex + 1;
        while (endOfSectionIndex < lines.Count && lines[endOfSectionIndex].Type != IniLineType.Section)
        {
            endOfSectionIndex++;
        }

        if (prefix == "+")
        {
            bool exists = false;
            for (int i = sectionIndex + 1; i < endOfSectionIndex; i++)
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
                lines.Insert(endOfSectionIndex, new IniLine
                {
                    Type = IniLineType.Entry,
                    Raw = $"+{key}={value} ; tempest",
                    SectionName = section,
                    Key = key,
                    Value = value + " ; tempest",
                    Prefix = "+"
                });
            }
        }
        else if (prefix == "-")
        {
            for (int i = sectionIndex + 1; i < endOfSectionIndex; i++)
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
            for (int i = sectionIndex + 1; i < endOfSectionIndex; i++)
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
            for (int i = sectionIndex + 1; i < endOfSectionIndex; i++)
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
                    Raw = $"{key}={value} ; tempest",
                    SectionName = section,
                    Key = key,
                    Value = value + " ; tempest",
                    Prefix = null
                };
            }
            else
            {
                lines.Insert(endOfSectionIndex, new IniLine
                {
                    Type = IniLineType.Entry,
                    Raw = $"{key}={value} ; tempest",
                    SectionName = section,
                    Key = key,
                    Value = value + " ; tempest",
                    Prefix = null
                });
            }
        }
    }
}
