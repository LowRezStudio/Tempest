using System.Diagnostics;
using Tempest.CLI.Launcher;

namespace Tempest.CLI.Extensions;

internal static class ProcessExtensions
{
    public static async Task<bool> InjectLibraryAsync(this Process process, string path, bool is64Bit)
    {
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

        Console.Error.WriteLine($"[inject] PID {pid}: {path}");
        var loaded = OperatingSystem.IsWindows()
            ? await RemoteLoadLibrary.InjectAsync(pid, path, is64Bit)
            : await RunInjectorAsync(pid, path, is64Bit);
        if (loaded)
            return true;

        // Defender often quarantines the mod DLL once the game tries to load it.
        if ((dllExisted && Antivirus.Removed(path)) || Antivirus.Refuses(path))
        {
            Console.Error.WriteLine($"[inject] {path} was blocked by an antivirus");
            Antivirus.Report(Path.GetDirectoryName(path)!);
        }
        return false;
    }

    /// <summary>Loads the DLL through inject32/64.exe, run in the game's Wine prefix.</summary>
    private static async Task<bool> RunInjectorAsync(int pid, string path, bool is64Bit)
    {
        string executable;
        try
        {
            executable = GetExecutablePath(is64Bit ? "inject64.exe" : "inject32.exe");
        }
        catch (FileNotFoundException ex)
        {
            Console.Error.WriteLine($"[inject] {ex.Message}");
            return false;
        }

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
