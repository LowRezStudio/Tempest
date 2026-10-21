using System.Collections.Concurrent;
using System.Diagnostics;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using Tempest.CLI.Launcher;

namespace Tempest.CLI.Extensions;

internal static class ProcessExtensions
{
    private static readonly string ToolsDir = TempestPathUtility.GetToolsDirectory();
    private static readonly ConcurrentDictionary<string, string> Extracted = new(StringComparer.OrdinalIgnoreCase);

    [DllImport("advapi32.dll", SetLastError = true)]
    private static extern bool OpenProcessToken(IntPtr ProcessHandle, uint DesiredAccess, out IntPtr TokenHandle);

    [DllImport("advapi32.dll", SetLastError = true)]
    private static extern bool LookupPrivilegeValue(string? lpSystemName, string lpName, out long lpLuid);

    [DllImport("advapi32.dll", SetLastError = true)]
    private static extern bool AdjustTokenPrivileges(IntPtr TokenHandle, bool DisableAllPrivileges, ref TokPriv1Luid NewState, uint BufferLength, IntPtr PreviousState, IntPtr ReturnLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(IntPtr hObject);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr GetCurrentProcess();

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct TokPriv1Luid
    {
        public uint PrivilegeCount;
        public long Luid;
        public uint Attributes;
    }

    private const uint SE_PRIVILEGE_ENABLED = 0x2;
    private const uint TOKEN_QUERY = 0x0008;
    private const uint TOKEN_ADJUST_PRIVILEGES = 0x0020;

    private static void EnableDebugPrivilege()
    {
        try
        {
            if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, out var tokenHandle))
                return;
            try
            {
                if (!LookupPrivilegeValue(null, "SeDebugPrivilege", out var luid))
                    return;
                var tp = new TokPriv1Luid { PrivilegeCount = 1, Luid = luid, Attributes = SE_PRIVILEGE_ENABLED };
                AdjustTokenPrivileges(tokenHandle, false, ref tp, (uint)System.Runtime.InteropServices.Marshal.SizeOf<TokPriv1Luid>(), IntPtr.Zero, IntPtr.Zero);
            }
            finally { CloseHandle(tokenHandle); }
        }
        catch { }
    }

    public static string GetOrExtractLoaderDll() => ExtractResource("tempest_loader.dll");

    public static async Task<bool> InjectLibraryAsync(this Process process, string path, bool is64Bit)
    {
        EnableDebugPrivilege();
        string executable;
        try
        {
            executable = ExtractResource(is64Bit ? "inject64.exe" : "inject32.exe");
        }
        // FileNotFound: the temp copy was quarantined before it could be moved into place.
        catch (IOException ex) when (Antivirus.IsBlock(ex) || ex is FileNotFoundException)
        {
            Console.Error.WriteLine($"[inject] antivirus blocked the injector in {ToolsDir}; allow the folder to load mod DLLs");
            Antivirus.Report(ToolsDir);
            return false;
        }

        if (Antivirus.Refuses(path))
        {
            Console.Error.WriteLine($"[inject] antivirus blocked {path}");
            Antivirus.Report(Path.GetDirectoryName(path)!);
            return false;
        }
        var dllExisted = File.Exists(path);

        // The game process (and its wineserver) may not be up yet when Proton/Wine is cold-starting
        // a prefix; poll until it shows up or the wrapper exits.
        var pid = 0;
        for (var i = 0; i < 90 && !process.HasExited; i++)
        {
            pid = await process.GetProcessId();
            if (pid > 0) break;
            await Task.Delay(TimeSpan.FromSeconds(1));
        }

        if (pid == 0)
        {
            Console.Error.WriteLine($"[inject] failed to find the game process (is it Wine/Proton?): {path}");
            return false;
        }

        Console.Error.WriteLine($"[inject] {Path.GetFileName(executable)} -> PID {pid}: {path}");
        var inject = new Process();
        inject.StartInfo.FileName = executable;
        inject.StartInfo.Arguments = $"{pid} \"{path}\"";
        inject.StartInfo.RedirectStandardError = true;
        inject.StartInfo.UseShellExecute = false;
        // A failed injection must not take the launch down with it: the game is already running
        // and the launcher only knows it is while this process waits on it.
        try
        {
            inject.UseWineBinary().Start();
        }
        catch (Exception ex) when (Antivirus.IsBlock(ex) || Antivirus.Removed(executable))
        {
            Console.Error.WriteLine($"[inject] antivirus blocked {executable}; allow its folder to load mod DLLs");
            Antivirus.Report(ToolsDir);
            return false;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"[inject] failed to start {Path.GetFileName(executable)}: {ex.Message}");
            return false;
        }

        var stderr = inject.StandardError.ReadToEndAsync();
        await inject.WaitForExitAsync();
        var err = await stderr;
        if (!string.IsNullOrEmpty(err))
            Console.Error.WriteLine($"[inject stderr]\n{err.TrimEnd()}");
        Console.Error.WriteLine($"[inject] exit code: {inject.ExitCode}");
        if (inject.ExitCode == 0)
            return true;

        // Defender often lets the injector start and then kills and quarantines it, or quarantines
        // the mod DLL once the game tries to load it.
        if (Antivirus.Removed(executable))
        {
            Console.Error.WriteLine($"[inject] {Path.GetFileName(executable)} was removed by an antivirus");
            Antivirus.Report(ToolsDir);
        }
        else if ((dllExisted && Antivirus.Removed(path)) || Antivirus.Refuses(path))
        {
            Console.Error.WriteLine($"[inject] {path} was blocked by an antivirus");
            Antivirus.Report(Path.GetDirectoryName(path)!);
        }
        return false;
    }

    // Defender's behaviour heuristics block a process that drops an exe into a fresh %TEMP% folder and runs it
    // against another process, so the helpers are written once to a stable, content-addressed path and reused.
    private static string ExtractResource(string fileName) => Extracted.GetOrAdd(fileName, static name =>
    {
        var asm = Assembly.GetExecutingAssembly();
        var resourceName = asm.GetManifestResourceNames()
            .FirstOrDefault(n => n.EndsWith(name, StringComparison.OrdinalIgnoreCase)) ?? throw new InvalidOperationException($"Embedded resource for '{name}' not found. Available resources: {string.Join(',', asm.GetManifestResourceNames())}");

        byte[] bytes;
        using (var rs = asm.GetManifestResourceStream(resourceName) ?? throw new InvalidOperationException($"Failed to open resource stream '{resourceName}'"))
        using (var ms = new MemoryStream())
        {
            rs.CopyTo(ms);
            bytes = ms.ToArray();
        }

        var dir = Path.Combine(ToolsDir, Convert.ToHexStringLower(SHA256.HashData(bytes))[..16]);
        var outPath = Path.Combine(dir, name);
        if (File.Exists(outPath))
            return outPath;

        Directory.CreateDirectory(dir);
        var tmpPath = $"{outPath}.{Guid.NewGuid():N}.tmp";
        File.WriteAllBytes(tmpPath, bytes);
        try
        {
            File.Move(tmpPath, outPath);
        }
        catch (IOException) when (File.Exists(outPath))
        {
            // another launch extracted it first
            File.Delete(tmpPath);
        }
        return outPath;
    });
}