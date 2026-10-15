using System.Diagnostics;
using System.Security.Cryptography;
using Microsoft.Win32;
using Tempest.CLI.Mods;

namespace Tempest.CLI.Build;

internal static class PrerequisiteInstaller
{
    // Paladins' installscript.vdf names these UE3 installers. Each includes the
    // game's runtime components; do not launch unrelated executables in Redist.
    internal static readonly (string File, string SteamKey)[] Installers =
    [
        ("UE3Redist_vs2012.exe", "PR7b"),
        ("UE3Redist_vs2010.exe", "PR7c"),
    ];

    internal static async Task InstallAsync(string path, bool force, bool dryRun)
    {
        if (!Directory.Exists(path) && !File.Exists(path)) throw new DirectoryNotFoundException("Game installation not found.");
        var game = GameFolderResolver.Resolve(Path.GetFullPath(path));
        var redist = Path.Combine(game, "Binaries", "Redist");
        var available = Installers.Where(i => File.Exists(Path.Combine(redist, i.File))).ToArray();
        if (available.Length == 0) throw new FileNotFoundException("This game installation is missing its bundled UE3 prerequisites. Verify its game files and retry.");
        if (dryRun)
        {
            foreach (var installer in available) Console.WriteLine(Path.Combine(redist, installer.File));
            return;
        }
        if (!OperatingSystem.IsWindows()) throw new PlatformNotSupportedException("Game prerequisite installation is currently supported on Windows.");

        var receipts = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Tempest", "prerequisites");
        Directory.CreateDirectory(receipts);
        // Serialize across launcher instances; closing/crashing releases this lock.
        await using var gate = await AcquireGateAsync(Path.Combine(receipts, "install.lock"));
        foreach (var installer in available)
        {
            var executable = Path.Combine(redist, installer.File);
            await using var input = File.OpenRead(executable);
            var hash = Convert.ToHexString(await SHA256.HashDataAsync(input));
            var receipt = Path.Combine(receipts, hash + ".installed");
            if (!force && (File.Exists(receipt) || SteamInstalled(installer.SteamKey)))
            {
                Console.WriteLine($"Already installed: {installer.File}");
                continue;
            }
            Console.WriteLine($"Installing {installer.File}. Complete the installer dialogs to continue.");
            // No quiet switch: retain the vendor's acceptance and elevation UI.
            using var process = Process.Start(new ProcessStartInfo(executable)
            {
                UseShellExecute = true,
                Verb = "runas",
                WorkingDirectory = redist,
            }) ?? throw new IOException($"Could not start {installer.File}.");
            await process.WaitForExitAsync();
            if (process.ExitCode is not (0 or 3010 or 1641))
                throw new IOException($"{installer.File} failed or was cancelled (exit {process.ExitCode}). Retry with Install game prerequisites in the instance menu.");
            await File.WriteAllTextAsync(receipt, installer.File);
            if (process.ExitCode is 3010 or 1641) Console.WriteLine("Windows must restart to finish installing the prerequisites.");
        }
    }

    private static bool SteamInstalled(string name)
    {
        if (!OperatingSystem.IsWindows()) return false;
        foreach (var view in new[] { RegistryView.Registry32, RegistryView.Registry64 })
        {
            using var hive = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, view);
            using var key = hive.OpenSubKey(@"SOFTWARE\HiRez Studios");
            if (key?.GetValue(name) is int value && value > 0) return true;
        }
        return false;
    }

    private static async Task<FileStream> AcquireGateAsync(string path)
    {
        var deadline = DateTime.UtcNow.AddMinutes(30);
        while (true)
        {
            try { return new FileStream(path, FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None); }
            catch (IOException) when (DateTime.UtcNow < deadline) { await Task.Delay(500); }
        }
    }
}
