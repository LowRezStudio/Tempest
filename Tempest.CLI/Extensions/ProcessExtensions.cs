using System.Diagnostics;
using System.Reflection;

namespace Tempest.CLI.Extensions;

internal static class ProcessExtensions
{
    private static readonly string TempExtractDir = Path.Combine(Path.GetTempPath(), "Tempest.Inject", Guid.NewGuid().ToString("N"));
    private static readonly object ExtractLock = new();
    private static string? _inject32Path;
    private static string? _inject64Path;

    public static async Task<bool> InjectLibraryAsync(this Process process, string path, bool is64Bit)
    {
        var executable = is64Bit ? GetOrExtractExecutable("inject64.exe") : GetOrExtractExecutable("inject32.exe");

        var pid = await process.GetProcessId();

        var inject = new Process();

        inject.StartInfo.FileName = executable;
        inject.StartInfo.Arguments = $"{pid} \"{path}\"";

        inject.UseWine().Start();
        await inject.WaitForExitAsync();

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
                .FirstOrDefault(n => n.EndsWith(fileName, StringComparison.OrdinalIgnoreCase));

            if (resourceName == null)
                throw new InvalidOperationException($"Embedded resource for '{fileName}' not found. Available resources: {string.Join(',', asm.GetManifestResourceNames())}");

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