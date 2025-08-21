using System.Diagnostics;

namespace Tempest.CLI;

internal static class ProcessUtility
{
    public static async Task<bool> InjectLibraryAsync(this Process process, string path, bool is64Bit)
    {
        var executable = Path.Join(AppContext.BaseDirectory, is64Bit ? "inject64.exe" : "inject32.exe");

        var pid = await process.GetProcessId();

        var inject = new Process();

        inject.StartInfo.FileName = executable;
        inject.StartInfo.Arguments = $"{pid} \"{path}\"";

        inject.UseWine().Start();
        await inject.WaitForExitAsync();

        return inject.ExitCode == 0;
    }
}