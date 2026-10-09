using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;
using ConsoleAppFramework;
using Tempest.CLI.Mods;

namespace Tempest.CLI.Cleanup;

internal class CleanupCommands
{
    /// <summary>Removes registered instances and their mods before launcher uninstallation</summary>
    /// <param name="manifest">Launcher cleanup manifest path</param>
    /// <param name="interactive">Show confirmation and legacy-instance classification dialogs on Windows</param>
    /// <param name="dryRun">Validate and print the plan without modifying files</param>
    /// <param name="confirm">Confirm cleanup without native dialogs (for the launcher wizard)</param>
    /// <param name="launcherSession">Cleanup invoked by the launcher's own wizard</param>
    public async Task Run(string manifest, bool interactive = false, bool dryRun = false, bool confirm = false, bool launcherSession = false)
    {
        try
        {
            if (!File.Exists(manifest))
            {
                if (!Directory.Exists(System.IO.Path.GetDirectoryName(System.IO.Path.GetFullPath(manifest)))) return;
                if (interactive && OperatingSystem.IsWindows())
                    throw new IOException("Open Tempest once to register its instances for cleanup, then retry uninstalling.");
                throw new FileNotFoundException("Cleanup manifest not found.");
            }
            manifest = System.IO.Path.GetFullPath(manifest);
            var registry = JsonSerializer.Deserialize(await File.ReadAllTextAsync(manifest), CleanupJsonContext.Default.CleanupManifest)
                ?? throw new InvalidDataException("Cleanup manifest is empty.");
            if (registry.Version != 1) throw new InvalidDataException("Unsupported cleanup manifest version.");
            if (registry.Instances.Count == 0) return;

            foreach (var instance in registry.Instances)
            {
                if (!Guid.TryParse(instance.Id, out _)) throw new InvalidDataException("Invalid instance ID.");
                ValidateRoot(instance.Path);
            }
            if (interactive && OperatingSystem.IsWindows())
            {
                foreach (var instance in registry.Instances.Where(i => i.Origin == null && i.ManagedPath == null))
                {
                    var answer = MessageBoxW(IntPtr.Zero,
                        $"Was this instance downloaded with Tempest?\n\n{instance.Label}\n{instance.Path}\n\nYes: delete its game folder and mods.\nNo: keep imported game files and remove Tempest mods.\nCancel: stop uninstalling.",
                        "Tempest instance cleanup", 0x123);
                    if (answer == 2) throw new OperationCanceledException("Uninstallation cancelled.");
                    if (answer == 6) instance.ManagedPath = instance.Path;
                    instance.Origin = answer == 6 ? "download" : "import";
                }
            }
            var groups = registry.Instances.GroupBy(i => ResolveRoot(i.Path), PathComparer).ToArray();
            foreach (var group in groups)
            {
                ValidateRoot(group.Key);
                var delete = DeleteGame(group);
                Console.WriteLine($"{(delete ? "Delete game folder and mods" : "Remove mods; keep game folder")}: {group.Key}");
                ValidateModPaths(group.Key);
            }
            // Never allow removing a registered child through a parent's recursive delete.
            foreach (var parent in groups.Where(DeleteGame))
                if (groups.Any(child => child.Key != parent.Key && IsWithin(child.Key, parent.Key)))
                    throw new IOException("Overlapping instance folders must be resolved before cleanup.");

            if (dryRun) return;
            if (interactive && OperatingSystem.IsWindows())
            {
                var summary = string.Join("\n", groups.Select(g => $"{(DeleteGame(g) ? "DELETE" : "KEEP game, remove mods")}: {g.Key}"));
                if (MessageBoxW(IntPtr.Zero, $"Remove all registered instances and their Tempest mods?\n\n{summary}\n\nDeleted game folders cannot be recovered. Shared system runtimes are kept.", "Uninstall Tempest", 0x124) != 6)
                    throw new OperationCanceledException("Uninstallation cancelled.");
            }
            else if (!confirm) throw new InvalidOperationException("Cleanup requires confirmation.");

            EnsureGameStopped(groups.Select(g => g.Key));
            if (!launcherSession && Process.GetProcessesByName("tempest-launcher").Length > 0)
                throw new IOException("Close Tempest before uninstalling so downloads and instance changes can finish.");
            foreach (var group in groups)
            {
                var root = group.Key;
                if (Directory.Exists(root))
                {
                    if (DeleteGame(group))
                    {
                        EnsureNoLinks(root, recursive: true);
                        Directory.Delete(root, recursive: true);
                    }
                    else await RemoveModsAsync(root);
                }
                foreach (var instance in group)
                {
                    var cache = System.IO.Path.Combine(System.IO.Path.GetDirectoryName(manifest)!, "instances", instance.Id);
                    EnsureNoLinks(cache, recursive: true);
                    if (Directory.Exists(cache)) Directory.Delete(cache, recursive: true);
                    registry.Instances.Remove(instance);
                    registry.RemovedIds.Add(instance.Id);
                }
                // Persist completed groups so a failed cleanup can resume safely.
                await SaveAsync(manifest, registry);
            }
            registry.Cleaned = true;
            await SaveAsync(manifest, registry);
        }
        catch (Exception error)
        {
            await Console.Error.WriteLineAsync(error.Message);
            if (interactive && OperatingSystem.IsWindows())
                MessageBoxW(IntPtr.Zero, error.Message, "Tempest cleanup did not finish", 0x10);
            Environment.ExitCode = 1;
        }
    }

    private static readonly StringComparer PathComparer = OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;
    private static bool DeleteGame(IEnumerable<CleanupInstance> instances) =>
        instances.All(i => i.Origin != "import" && i.ManagedPath != null && PathComparer.Equals(ResolveRoot(i.Path), System.IO.Path.GetFullPath(i.ManagedPath).TrimEnd(System.IO.Path.DirectorySeparatorChar)));

    private static string ResolveRoot(string path)
    {
        var full = System.IO.Path.GetFullPath(path).TrimEnd(System.IO.Path.DirectorySeparatorChar);
        try { return GameFolderResolver.Resolve(full); }
        catch (DirectoryNotFoundException) { return full; }
    }

    private static void ValidateRoot(string path)
    {
        if (string.IsNullOrWhiteSpace(path) || !System.IO.Path.IsPathFullyQualified(path)) throw new InvalidDataException("Instance path must be absolute.");
        var full = System.IO.Path.GetFullPath(path).TrimEnd(System.IO.Path.DirectorySeparatorChar);
        if (System.IO.Path.GetDirectoryName(full) == null || new[] { Environment.SpecialFolder.UserProfile, Environment.SpecialFolder.ApplicationData, Environment.SpecialFolder.LocalApplicationData, Environment.SpecialFolder.Windows }.Any(f => PathComparer.Equals(full, Environment.GetFolderPath(f))))
            throw new IOException("Refusing to clean a system or profile root.");
        EnsureNoLinks(full);
    }

    private static bool IsWithin(string file, string root) =>
        System.IO.Path.GetFullPath(file).StartsWith(root + System.IO.Path.DirectorySeparatorChar, OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal);

    private static void EnsureNoLinks(string path, bool recursive = false)
    {
        for (var current = path; current != null; current = System.IO.Path.GetDirectoryName(current))
            if ((File.Exists(current) || Directory.Exists(current)) && (File.GetAttributes(current) & FileAttributes.ReparsePoint) != 0)
                throw new IOException($"Refusing to follow a link or junction: {current}");
        if (!recursive || !Directory.Exists(path)) return;
        foreach (var entry in Directory.EnumerateFileSystemEntries(path))
        {
            if ((File.GetAttributes(entry) & FileAttributes.ReparsePoint) != 0) throw new IOException($"Refusing to follow a link or junction: {entry}");
            if (Directory.Exists(entry)) EnsureNoLinks(entry, recursive: true);
        }
    }

    private static void ValidateModPaths(string root)
    {
        if (!Directory.Exists(root)) return;
        EnsureNoLinks(System.IO.Path.Combine(root, ".tempest"), recursive: true);
        var metadata = System.IO.Path.Combine(root, ".tempest", "mods", "mods.json");
        if (!File.Exists(metadata)) return;
        // LoadMetadata deliberately tolerates malformed JSON. Cleanup must reject it.
        _ = JsonSerializer.Deserialize(File.ReadAllText(metadata), ModSourceGenerationContext.Default.ListModRecord)
            ?? throw new InvalidDataException("Invalid mod metadata.");
        foreach (var mod in ModCommands.LoadMetadata(root))
        {
            if (string.IsNullOrEmpty(mod.Id) || mod.Id is "." or ".." || mod.Id.IndexOfAny(['/', '\\']) >= 0)
                throw new InvalidDataException("Invalid mod ID.");
            foreach (var file in mod.InstalledFiles.Concat(mod.OwnedFiles))
            {
                if (!IsWithin(file, root)) throw new IOException($"Mod path escapes its game folder: {file}");
                EnsureNoLinks(file);
            }
        }
    }

    private static async Task RemoveModsAsync(string root)
    {
        var mods = ModCommands.LoadMetadata(root);
        foreach (var mod in mods.ToArray().Reverse())
        {
            IModInstaller installer = mod.Kind.Equals("V2", StringComparison.OrdinalIgnoreCase)
                ? new ModV2Installer(strictRemoval: true) : new ModV1Installer(strictRemoval: true);
            await installer.RemoveAsync(root, mod);
            mods.Remove(mod);
            var metadata = ModCommands.GetMetadataPath(root);
            await File.WriteAllTextAsync(metadata, JsonSerializer.Serialize(mods, ModSourceGenerationContext.Default.ListModRecord));
        }
        var local = System.IO.Path.Combine(root, ".tempest");
        if (Directory.Exists(local)) Directory.Delete(local, recursive: true);
    }

    private static void EnsureGameStopped(IEnumerable<string> roots)
    {
        if (!OperatingSystem.IsWindows()) return;
        foreach (var process in Process.GetProcessesByName("Paladins"))
        {
            using (process)
            {
                var executable = process.MainModule?.FileName;
                if (executable == null || roots.Any(root => IsWithin(executable, root)))
                    throw new IOException("Close Paladins before removing instances.");
            }
        }
    }

    private static async Task SaveAsync(string path, CleanupManifest value)
    {
        var temporary = path + ".tmp";
        await File.WriteAllTextAsync(temporary, JsonSerializer.Serialize(value, CleanupJsonContext.Default.CleanupManifest));
        File.Move(temporary, path, overwrite: true);
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int MessageBoxW(IntPtr window, string text, string caption, uint type);
}
