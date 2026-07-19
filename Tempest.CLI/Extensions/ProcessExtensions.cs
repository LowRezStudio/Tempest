using System.Diagnostics;
using System.Reflection;
using System.Runtime.InteropServices;

namespace Tempest.CLI.Extensions;

internal static class ProcessExtensions
{
    private static readonly string TempExtractDir = Path.Combine(Path.GetTempPath(), "Tempest.Inject", Guid.NewGuid().ToString("N"));
    private static readonly Lock ExtractLock = new();
    private static string? _inject32Path;
    private static string? _inject64Path;
    private static string? _loaderDllPath;

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

    public static string GetOrExtractLoaderDll()
    {
        if (_loaderDllPath is not null && File.Exists(_loaderDllPath))
            return _loaderDllPath;
        lock (ExtractLock)
        {
            if (_loaderDllPath is not null && File.Exists(_loaderDllPath))
                return _loaderDllPath;
            Directory.CreateDirectory(TempExtractDir);
            var asm = Assembly.GetExecutingAssembly();
            var resourceName = asm.GetManifestResourceNames()
                .FirstOrDefault(n => n.EndsWith("tempest_loader.dll", StringComparison.OrdinalIgnoreCase))
                ?? throw new InvalidOperationException("Embedded resource 'tempest_loader.dll' not found.");
            var outPath = Path.Combine(TempExtractDir, "tempest_loader.dll");
            using (var rs = asm.GetManifestResourceStream(resourceName)!)
            using (var fs = File.Create(outPath)) { rs.CopyTo(fs); fs.Flush(true); }
            _loaderDllPath = outPath;
            return outPath;
        }
    }

    public static async Task<bool> InjectLibraryAsync(this Process process, string path, bool is64Bit)
    {
        EnableDebugPrivilege();
        var executable = is64Bit ? GetOrExtractExecutable("inject64.exe") : GetOrExtractExecutable("inject32.exe");
        var pid = await process.GetProcessId();
        Console.Error.WriteLine($"[inject] {Path.GetFileName(executable)} -> PID {pid}: {path}");
        var inject = new Process();
        inject.StartInfo.FileName = executable;
        inject.StartInfo.Arguments = $"{pid} \"{path}\"";
        inject.StartInfo.RedirectStandardError = true;
        inject.StartInfo.UseShellExecute = false;
        inject.UseWine().Start();
        var stderr = inject.StandardError.ReadToEndAsync();
        await inject.WaitForExitAsync();
        var err = await stderr;
        if (!string.IsNullOrEmpty(err))
            Console.Error.WriteLine($"[inject stderr]\n{err.TrimEnd()}");
        Console.Error.WriteLine($"[inject] exit code: {inject.ExitCode}");
        return inject.ExitCode == 0;
    }

    private static string GetOrExtractExecutable(string fileName)
    {
        // Return cached path if already extracted
        if (fileName.EndsWith("inject32.exe", StringComparison.OrdinalIgnoreCase) && _inject32Path is not null)
            return _inject32Path;
        if (fileName.EndsWith("inject64.exe", StringComparison.OrdinalIgnoreCase) && _inject64Path is not null)
            return _inject64Path;

        lock (ExtractLock)
        {
            // double-check in lock
            if (fileName.EndsWith("inject32.exe", StringComparison.OrdinalIgnoreCase) && _inject32Path is not null)
                return _inject32Path;
            if (fileName.EndsWith("inject64.exe", StringComparison.OrdinalIgnoreCase) && _inject64Path is not null)
                return _inject64Path;

            Directory.CreateDirectory(TempExtractDir);

            var asm = Assembly.GetExecutingAssembly();
            var resourceName = asm.GetManifestResourceNames()
                .FirstOrDefault(n => n.EndsWith(fileName, StringComparison.OrdinalIgnoreCase)) ?? throw new InvalidOperationException($"Embedded resource for '{fileName}' not found. Available resources: {string.Join(',', asm.GetManifestResourceNames())}");
            var outPath = Path.Combine(TempExtractDir, fileName);

            using (var rs = asm.GetManifestResourceStream(resourceName) ?? throw new InvalidOperationException($"Failed to open resource stream '{resourceName}'"))
            using (var fs = File.Create(outPath))
            {
                rs.CopyTo(fs);
                fs.Flush(true);
            }

            if (fileName.EndsWith("inject32.exe", StringComparison.OrdinalIgnoreCase))
                _inject32Path = outPath;
            else
                _inject64Path = outPath;

            return outPath;
        }
    }
}