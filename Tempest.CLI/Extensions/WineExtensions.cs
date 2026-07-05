using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;

namespace Tempest.CLI.Extensions;

internal static class WineExtensions
{
    public static Process UseWine(this Process process)
    {
        // You obviously don't need wine on Windows
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows)) return process;

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
        process.StartInfo.EnvironmentVariables["NATIVE_FILENAME"] = process.StartInfo.FileName;

        process.StartInfo.FileName = Environment.GetEnvironmentVariable("WINE") ?? "wine";

        if (process.StartInfo.ArgumentList.Count > 0)
        {
            process.StartInfo.ArgumentList.Insert(0, filename);
        }
        else
        {
            process.StartInfo.Arguments = $"\"{filename.Replace("\"", "\\\"")}\" {process.StartInfo.Arguments}";
        }

        return process;
    }

    public static Process UseGamescope(this Process process, string? gamescopeArgs = "-f --force-grab-cursor")
    {
        // Gamescope is a Linux-only compositor
        if (!OperatingSystem.IsLinux()) return process;

        var filename = process.StartInfo.FileName;
        var extraArgs = string.IsNullOrWhiteSpace(gamescopeArgs)
            ? []
            : gamescopeArgs.Split(' ', StringSplitOptions.RemoveEmptyEntries);

        if (process.StartInfo.ArgumentList.Count > 0)
        {
            var originalArgs = process.StartInfo.ArgumentList.ToArray();
            process.StartInfo.ArgumentList.Clear();
            foreach (var arg in extraArgs) process.StartInfo.ArgumentList.Add(arg);
            process.StartInfo.ArgumentList.Add("--");
            process.StartInfo.ArgumentList.Add(filename);
            foreach (var arg in originalArgs) process.StartInfo.ArgumentList.Add(arg);
            process.StartInfo.FileName = "gamescope";
        }
        else
        {
            var prefix = extraArgs.Length > 0 ? string.Join(" ", extraArgs) + " " : "";
            process.StartInfo.Arguments = $"{prefix}-- {filename} {process.StartInfo.Arguments}";
            process.StartInfo.FileName = "gamescope";
        }

        return process;
    }

    public static async Task<int> GetProcessId(this Process process)
    {
        // You obviously don't need any of this for native platforms
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows) || (process.StartInfo.FileName != "wine" && process.StartInfo.FileName != "gamescope"))
            return process.Id;

        var filename = Path.GetFileName(process.StartInfo.EnvironmentVariables["NATIVE_FILENAME"]) ?? throw new Exception("'NATIVE_FILENAME' Environment Variable is somehow null, this isn't your fault.");
        return await GetProcessId(filename);
    }

    public static string GetPrefix()
    {
        var envPrefix = Environment.GetEnvironmentVariable("WINEPREFIX");
        if (!string.IsNullOrEmpty(envPrefix))
        {
            return Path.GetFullPath(envPrefix);
        }
        return Path.GetFullPath(TempestPathUtility.GetWinePrefixDirectory());
    }

    public static Task WaitForPrefix()
    {
        var process = new Process();

        process.StartInfo.FileName = "cmd.exe";
        process.StartInfo.Arguments = "/c echo DONE";

        process.UseWine().Start();

        return process.WaitForExitAsync();
    }

    public static async Task<int> GetProcessId(string processName)
    {
        var process = new Process();

        process.StartInfo.FileName = "tasklist.exe";
        process.StartInfo.Arguments = "/fo csv";
        process.StartInfo.RedirectStandardOutput = true;
        process.StartInfo.CreateNoWindow = true;

        process.UseWine().Start();

        string output = await process.StandardOutput.ReadToEndAsync();
        await process.WaitForExitAsync();

        var pids = new List<int>();

        foreach (var line in output.Split('\n'))
        {
            if (line.Contains($"\"{processName}\"", StringComparison.OrdinalIgnoreCase))
            {
                // Split by quotes and get the PID (should be the next quoted value after process name)
                var parts = line.Split('"');
                for (int i = 0; i < parts.Length - 1; i++)
                {
                    if (parts[i].Equals(processName, StringComparison.OrdinalIgnoreCase) && i + 2 < parts.Length)
                    {
                        if (int.TryParse(parts[i + 2], out int pid))
                        {
                            pids.Add(pid);
                            break;
                        }
                    }
                }
            }
        }

        if (pids.Count == 0) return 0;
        if (pids.Count == 1) return pids[0];

        int newestPid = pids[0];
        DateTime? newestTime = null;

        foreach (var pid in pids)
        {
            var startTime = await GetProcessStartTime(pid);
            if (startTime != null && (newestTime == null || startTime > newestTime))
            {
                newestTime = startTime;
                newestPid = pid;
            }
        }

        return newestPid;
    }

    public static async Task<bool> IsWinePidAlive(int pid)
    {
        var process = new Process();

        process.StartInfo.FileName = "tasklist.exe";
        process.StartInfo.Arguments = $"/FI \"PID eq {pid}\" /fo csv";
        process.StartInfo.RedirectStandardOutput = true;
        process.StartInfo.CreateNoWindow = true;

        process.UseWine().Start();

        string output = await process.StandardOutput.ReadToEndAsync();
        await process.WaitForExitAsync();

        return output.Contains($"\"{pid}\"");
    }

    private static string[] ParseCsvLine(string line)
    {
        var result = new List<string>();
        bool inQuotes = false;
        var currentField = new StringBuilder();

        for (int i = 0; i < line.Length; i++)
        {
            char c = line[i];

            if (c == '"')
            {
                inQuotes = !inQuotes;
            }
            else if (c == ',' && !inQuotes)
            {
                result.Add(currentField.ToString());
                currentField.Clear();
            }
            else
            {
                currentField.Append(c);
            }
        }

        result.Add(currentField.ToString());
        return [.. result];
    }

    private static async Task<DateTime?> GetProcessStartTime(int pid)
    {
        try
        {
            var process = new Process();

            process.StartInfo.FileName = "wmic.exe";
            process.StartInfo.Arguments = $"process where processid={pid} get creationdate /format:csv";
            process.StartInfo.UseShellExecute = false;
            process.StartInfo.RedirectStandardOutput = true;
            process.StartInfo.CreateNoWindow = true;

            process.UseWine().Start();

            string output = await process.StandardOutput.ReadToEndAsync();
            await process.WaitForExitAsync();

            // Parse WMIC output to get creation date
            var lines = output.Split('\n', StringSplitOptions.RemoveEmptyEntries);
            foreach (var line in lines)
            {
                if (line.Contains("CreationDate") && !line.StartsWith("Node"))
                {
                    var parts = line.Split(',');
                    if (parts.Length >= 2)
                    {
                        var dateStr = parts[1].Trim();
                        // WMIC format: 20241221143022.123456-480
                        if (dateStr.Length >= 14)
                        {
                            var year = int.Parse(dateStr[..4]);
                            var month = int.Parse(dateStr.Substring(4, 2));
                            var day = int.Parse(dateStr.Substring(6, 2));
                            var hour = int.Parse(dateStr.Substring(8, 2));
                            var minute = int.Parse(dateStr.Substring(10, 2));
                            var second = int.Parse(dateStr.Substring(12, 2));

                            return new DateTime(year, month, day, hour, minute, second);
                        }
                    }
                }
            }
        }
        catch
        {
            // Fallback: assume it's the current time (not ideal but better than nothing)
            return DateTime.Now;
        }

        return null;
    }
}
