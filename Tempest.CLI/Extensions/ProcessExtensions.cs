using System.Diagnostics;
using System.Runtime.InteropServices;

namespace Tempest.CLI.Extensions;

internal static class ProcessExtensions
{
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

    public static async Task<bool> InjectLibraryAsync(this Process process, string path, bool is64Bit)
    {
        EnableDebugPrivilege();
        var executable = GetExecutablePath(is64Bit ? "inject64.exe" : "inject32.exe");

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
        inject.UseWineBinary().Start();
        var stderr = inject.StandardError.ReadToEndAsync();
        await inject.WaitForExitAsync();
        var err = await stderr;
        if (!string.IsNullOrEmpty(err))
            Console.Error.WriteLine($"[inject stderr]\n{err.TrimEnd()}");
        Console.Error.WriteLine($"[inject] exit code: {inject.ExitCode}");
        return inject.ExitCode == 0;
    }

    private static string GetExecutablePath(string fileName)
    {
        // copied next to the CLI by Tempest.CLI.csproj (build and publish)
        var path = Path.Combine(AppContext.BaseDirectory, fileName);
        return File.Exists(path)
            ? path
            : throw new FileNotFoundException($"'{fileName}' is missing next to the CLI ({AppContext.BaseDirectory}).", path);
    }
}