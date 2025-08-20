using System.Diagnostics;
using System.Runtime.InteropServices;

namespace Tempest.CLI;

internal static class WineUtility
{
    public static Process UseWine(this Process process)
    {
        // You obviously don't need wine on Windows
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            return process;

        var prefix = GetPrefix();
        var filename = process.StartInfo.FileName;

        if (!Directory.Exists(prefix))
        {
            Directory.CreateDirectory(prefix);
        }

        process.StartInfo.EnvironmentVariables["WINEPREFIX"] = prefix;

        // disable all kinds of logs
        process.StartInfo.EnvironmentVariables["DXVK_LOG_LEVEL"] = "none";
        process.StartInfo.EnvironmentVariables["DXVK_LOG_PATH"] = "/dev/null";
        process.StartInfo.EnvironmentVariables["WINEDEBUG"] = "-all";

        process.StartInfo.FileName = "wine";

        if (process.StartInfo.ArgumentList.Count > 0)
        {
            process.StartInfo.ArgumentList.Insert(0, filename);
        }
        else
        {
            process.StartInfo.Arguments = $"{filename} {process.StartInfo.Arguments}";
        }
        
        return process;
    }
    
    public static string GetPrefix() => Path.Join(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
        ".tempest", "prefix");

    public static Task WaitForPrefix()
    {
        var process = new Process();

        process.StartInfo.FileName = "cmd.exe";
        process.StartInfo.Arguments = "/c echo DONE";
        
        process.UseWine().Start();
        
        return process.WaitForExitAsync();
    }
}