using System.Diagnostics;
using System.Text.Json;
using ConsoleAppFramework;
using Tempest.CLI.Mods;

namespace Tempest.CLI.Instances;

internal class InstanceCommands
{
    /// <summary>Resolves instance paths to their canonical game folders</summary>
    /// <param name="paths">Base64-encoded UTF-8 JSON array of instance paths</param>
    public void Roots([Argument] string paths)
    {
        var values = JsonSerializer.Deserialize(System.Text.Encoding.UTF8.GetString(Convert.FromBase64String(paths)), InstanceJsonContext.Default.StringArray)
            ?? throw new InvalidDataException("Instance paths are missing.");
        Console.WriteLine(JsonSerializer.Serialize(values.Select(ResolveRoot).ToArray(), InstanceJsonContext.Default.StringArray));
    }

    /// <summary>Copies game files, installed mods and backups into an independent instance</summary>
    /// <param name="path">Source instance folder</param>
    /// <param name="output">New destination folder; must not exist</param>
    public void Clone([Argument] string path, string output)
    {
        string? stage = null;
        var committed = false;
        try
        {
            var source = GameFolderResolver.Resolve(Path.GetFullPath(path)).TrimEnd(Path.DirectorySeparatorChar);
            output = Path.GetFullPath(output).TrimEnd(Path.DirectorySeparatorChar);
            ValidateDestination(source, output);
            EnsureStopped(source);
            var metadata = Path.Combine(source, ".tempest", "mods", "mods.json");
            NoLinks(metadata);
            var mods = File.Exists(metadata)
                ? JsonSerializer.Deserialize(File.ReadAllText(metadata), ModSourceGenerationContext.Default.ListModRecord)
                    ?? throw new InvalidDataException("Invalid mod metadata.") : [];
            foreach (var mod in mods)
            {
                if (string.IsNullOrEmpty(mod.Id) || mod.Id is "." or ".." || mod.Id.IndexOfAny(['/', '\\']) >= 0)
                    throw new InvalidDataException("Invalid mod ID.");
                mod.InstalledFiles = mod.InstalledFiles.Select(p => Rebase(p, source, output, required: true)).ToList();
                mod.OwnedFiles = mod.OwnedFiles.Select(p => Rebase(p, source, output, required: true)).ToList();
                mod.OriginalPath = Rebase(mod.OriginalPath, source, output);
            }
            stage = StageCopy(source, output);
            if (File.Exists(metadata))
                File.WriteAllText(Path.Combine(stage, ".tempest", "mods", "mods.json"), JsonSerializer.Serialize(mods, ModSourceGenerationContext.Default.ListModRecord));
            Directory.Move(stage, output);
            stage = null;
            committed = true;
            Console.WriteLine(JsonSerializer.Serialize(new CloneResult(source, output), InstanceJsonContext.Default.CloneResult));
        }
        catch
        {
            if (committed) DeleteOwnedTree(output);
            throw;
        }
        finally
        {
            if (stage != null) DeleteOwnedTree(stage);
        }
    }

    internal static string ResolveRoot(string path)
    {
        var full = Path.GetFullPath(path);
        try { return GameFolderResolver.Resolve(full).TrimEnd(Path.DirectorySeparatorChar); }
        catch (DirectoryNotFoundException) { return full.TrimEnd(Path.DirectorySeparatorChar); }
    }

    private static readonly StringComparison Comparison = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
    private static bool Within(string path, string root) => Path.GetFullPath(path).StartsWith(root + Path.DirectorySeparatorChar, Comparison);

    private static string Rebase(string path, string source, string target, bool required = false)
    {
        if (required && !Path.IsPathRooted(path)) path = Path.GetFullPath(Path.Combine(source, path));
        if (Path.IsPathFullyQualified(path) && Within(path, source)) return Path.Combine(target, Path.GetRelativePath(source, path));
        if (required) throw new IOException($"Mod path escapes its instance: {path}");
        return path;
    }

    private static void ValidateDestination(string source, string target)
    {
        if (Directory.Exists(target) || File.Exists(target)) throw new IOException("The destination already exists. Choose a new folder.");
        if (source.Equals(target, Comparison) || Within(target, source) || Within(source, target)) throw new IOException("Source and destination folders must not overlap.");
        NoLinks(source);
        NoLinks(target);
    }

    private static string StageCopy(string source, string target)
    {
        if (!Directory.Exists(source)) throw new DirectoryNotFoundException("Source instance not found.");
        var parent = Path.GetDirectoryName(target) ?? throw new IOException("Invalid destination folder.");
        Directory.CreateDirectory(parent);
        var stage = target + ".copy-" + Guid.NewGuid().ToString("N");
        Directory.CreateDirectory(stage);
        try
        {
            CopyDirectory(source, stage);
            return stage;
        }
        catch { DeleteOwnedTree(stage); throw; }
    }

    private static void CopyDirectory(string source, string target)
    {
        foreach (var entry in Directory.EnumerateFileSystemEntries(source))
        {
            if ((File.GetAttributes(entry) & FileAttributes.ReparsePoint) != 0) throw new IOException($"Cannot copy a link or junction: {entry}");
            var destination = Path.Combine(target, Path.GetFileName(entry));
            if (Directory.Exists(entry))
            {
                Directory.CreateDirectory(destination);
                CopyDirectory(entry, destination);
            }
            else
            {
                var before = new FileInfo(entry);
                var length = before.Length;
                var modified = before.LastWriteTimeUtc;
                File.Copy(entry, destination);
                File.SetAttributes(destination, File.GetAttributes(destination) & ~FileAttributes.ReadOnly);
                var after = new FileInfo(entry);
                if (after.Length != length || after.LastWriteTimeUtc != modified) throw new IOException($"Source changed while copying: {entry}");
            }
        }
    }

    private static void NoLinks(string path)
    {
        for (var current = path; current != null; current = Path.GetDirectoryName(current))
            if ((Directory.Exists(current) || File.Exists(current)) && (File.GetAttributes(current) & FileAttributes.ReparsePoint) != 0)
                throw new IOException($"Cannot copy a link or junction: {current}");
    }

    private static void DeleteOwnedTree(string path)
    {
        NoLinks(path);
        if (Directory.Exists(path)) Directory.Delete(path, recursive: true);
    }

    private static void EnsureStopped(string root)
    {
        if (!OperatingSystem.IsWindows()) return;
        foreach (var process in Process.GetProcessesByName("Paladins"))
        {
            using (process)
                if (process.MainModule?.FileName is not string exe || Within(exe, root))
                    throw new IOException("Close games using this folder before copying it.");
        }
    }
}

internal record CloneResult(string Source, string Output);

[System.Text.Json.Serialization.JsonSerializable(typeof(CloneResult))]
[System.Text.Json.Serialization.JsonSerializable(typeof(string[]))]
internal partial class InstanceJsonContext : System.Text.Json.Serialization.JsonSerializerContext;
