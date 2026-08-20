using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;

namespace Tempest.CLI.Extensions;

/// <summary>
/// Wine runtime mode used by the most recent game launch.
/// Helper processes (tasklist, injector, ...) must use the same wine as the game
/// so they join the same wineserver, so UseWine() records the mode it chose and
/// UseWineBinary() follows it.
/// </summary>
internal enum WineRuntimeMode
{
    Auto,
    SystemWine,
    Proton,
}

internal static class WineExtensions
{
    /// <summary>Runtime chosen for the last game process (set by UseWine).</summary>
    private static WineRuntimeMode _gameRuntime = WineRuntimeMode.Auto;

    public static Process UseWine(this Process process, bool forceSystemWine = false, bool useSteamRuntime = true)
    {
        // You obviously don't need wine on Windows
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows)) return process;

        var protonDir = forceSystemWine ? null : ResolveProtonDirectory();

        // A server launch should stick to plain Wine (Proton is overkill for it) unless
        // no system Wine is available at all.
        if (forceSystemWine && !IsSystemWineAvailable())
        {
            Console.Error.WriteLine("Warning: system Wine was not found; falling back to Proton for this launch.");
            protonDir = ResolveProtonDirectory();
        }

        if (protonDir != null)
        {
            _gameRuntime = WineRuntimeMode.Proton;
            return UseProton(process, protonDir, useSteamRuntime);
        }

        _gameRuntime = WineRuntimeMode.SystemWine;
        return UseSystemWine(process);
    }

    /// <summary>
    /// Applies the runtime chosen for the current game to a *helper* process
    /// (cmd.exe, tasklist.exe, wmic.exe, inject32/64.exe). In Proton mode helpers run
    /// through Proton's own wine binary so they share the game's wineserver.
    /// </summary>
    public static Process UseWineBinary(this Process process)
    {
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows)) return process;

        if (ResolveRuntime() == WineRuntimeMode.Proton && ResolveProtonDirectory() is { } protonDir)
        {
            return UseProtonWine(process, protonDir);
        }

        return UseSystemWine(process);
    }

    private static Process UseSystemWine(Process process)
    {
        var prefix = GetPrefix();
        var filename = process.StartInfo.FileName;

        if (!Directory.Exists(prefix))
        {
            Directory.CreateDirectory(prefix);
        }

        process.StartInfo.EnvironmentVariables["WINEPREFIX"] = prefix;

        SetCommonWineEnvironment(process);

        process.StartInfo.FileName = Environment.GetEnvironmentVariable("WINE") ?? "wine";

        InsertExecutableFirst(process, filename);

        return process;
    }

    private static Process UseProton(Process process, string protonDir, bool useSteamRuntime)
    {
        var dataPath = EnsureProtonCompatDataPath(GetPrefix());
        var filename = process.StartInfo.FileName;

        if (!Directory.Exists(dataPath))
        {
            Directory.CreateDirectory(dataPath);
        }

        process.StartInfo.EnvironmentVariables["STEAM_COMPAT_DATA_PATH"] = dataPath;
        // Same prefix for helper processes so they connect to the game's wineserver.
        process.StartInfo.EnvironmentVariables["WINEPREFIX"] = Path.Combine(dataPath, "pfx");

        if (GetSteamRoot() is { } steamRoot)
        {
            process.StartInfo.EnvironmentVariables["STEAM_COMPAT_CLIENT_INSTALL_PATH"] = steamRoot;
        }

        // Always report the game as "playing" to the Steam client (rich presence) by registering
        // the Steam AppId through Proton's lsteamclient bridge. Needs Steam running to take effect.
        if (IsSteamRunning())
        {
            var appId = TempestPathUtility.SteamAppId.ToString();
            process.StartInfo.EnvironmentVariables["SteamAppId"] = appId;
            process.StartInfo.EnvironmentVariables["SteamGameId"] = appId;
            process.StartInfo.EnvironmentVariables["SteamOverlayGameId"] = appId;
            Console.Error.WriteLine($"Reporting game to Steam as AppId {appId}");
        }
        else
        {
            Console.Error.WriteLine("Steam isn't running, so the game won't be shown as playing on Steam.");
        }

        SetCommonWineEnvironment(process);

        var originalArgs = process.StartInfo.ArgumentList.ToArray();
        process.StartInfo.ArgumentList.Clear();

        // Optionally run Proton inside Steam's Linux Runtime (pressure-vessel).
        var command = Path.Combine(protonDir, "proton");
        if (useSteamRuntime && OperatingSystem.IsLinux() && ResolveSteamLinuxRuntime() is { } slrRun)
        {
            if (IsSteamRuntimeCompatible(slrRun))
            {
                process.StartInfo.FileName = slrRun;
                process.StartInfo.ArgumentList.Add("--");
                process.StartInfo.ArgumentList.Add(command);
            }
            else
            {
                Console.Error.WriteLine(
                    "Warning: Steam Linux Runtime is not compatible with the selected Proton build (needs Python 3.11+), " +
                    "launching Proton directly instead.");
                process.StartInfo.FileName = command;
            }
        }
        else
        {
            process.StartInfo.FileName = command;
        }

        process.StartInfo.ArgumentList.Add("waitforexitandrun");
        process.StartInfo.ArgumentList.Add(filename);
        foreach (var arg in originalArgs) process.StartInfo.ArgumentList.Add(arg);

        return process;
    }

    private static Process UseProtonWine(Process process, string protonDir)
    {
        var prefix = GetPrefix();
        var dataPath = prefix;
        var winePrefix = Path.Combine(dataPath, "pfx");
        var filename = process.StartInfo.FileName;

        if (!Directory.Exists(winePrefix))
        {
            Directory.CreateDirectory(winePrefix);
        }

        process.StartInfo.EnvironmentVariables["WINEPREFIX"] = winePrefix;

        SetCommonWineEnvironment(process);

        var wineBinary = GetProtonWineBinary(protonDir);
        if (wineBinary == null)
        {
            throw new Exception($"Couldn't find a wine binary in Proton directory '{protonDir}' (looked for files/bin/wine64 and files/bin/wine)");
        }

        process.StartInfo.FileName = wineBinary;

        // Proton's wine needs its bundled libraries.
        var libDirs = new[]
        {
            Path.Combine(protonDir, "files", "lib", "x86_64-linux-gnu"),
            Path.Combine(protonDir, "files", "lib", "i386-linux-gnu"),
        };
        var hasLdPath = process.StartInfo.EnvironmentVariables.ContainsKey("LD_LIBRARY_PATH");
        var existingLdPath = hasLdPath ? process.StartInfo.EnvironmentVariables["LD_LIBRARY_PATH"] : null;
        var extra = string.Join(':', libDirs.Where(Directory.Exists));
        process.StartInfo.EnvironmentVariables["LD_LIBRARY_PATH"] = !hasLdPath || string.IsNullOrEmpty(existingLdPath)
            ? extra
            : $"{extra}:{existingLdPath}";

        InsertExecutableFirst(process, filename);

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

    /// <summary>Set while KillProcessTree is sweeping /proc, so the main launch path doesn't exit early when the wrapper dies mid-sweep.</summary>
    internal static volatile bool KillInProgress;

    public static async Task KillProcessTree(Process process)
    {
        KillInProgress = true;
        try
        {
            var markers = new List<string>();
            if (process.StartInfo.EnvironmentVariables.ContainsKey("STEAM_COMPAT_DATA_PATH") &&
                process.StartInfo.EnvironmentVariables["STEAM_COMPAT_DATA_PATH"] is { } compat && !string.IsNullOrEmpty(compat))
                markers.Add(compat);
            if (process.StartInfo.EnvironmentVariables.ContainsKey("WINEPREFIX") &&
                process.StartInfo.EnvironmentVariables["WINEPREFIX"] is { } wpref && !string.IsNullOrEmpty(wpref))
                markers.Add(wpref);

            if (OperatingSystem.IsLinux() && markers.Count > 0)
            {
                foreach (var procDir in Directory.EnumerateDirectories("/proc"))
                {
                    var pidText = Path.GetFileName(procDir);
                    if (!int.TryParse(pidText, out var pid) || pid <= 0 || pid == process.Id) continue;

                    try
                    {
                        var env = File.ReadAllText(Path.Combine(procDir, "environ"));
                        if (markers.Any(env.Contains))
                        {
                            Process.GetProcessById(pid).Kill(true);
                        }
                    }
                    catch
                    {
                        // process already gone or not ours to read
                    }
                }
            }
        }
        finally
        {
            KillInProgress = false;
        }

        try
        {
            process.Kill(true);
        }
        catch
        {
            // already gone
        }

        try
        {
            await process.WaitForExitAsync();
        }
        catch
        {
            // ignore
        }
    }

    public static async Task<int> GetProcessId(this Process process)
    {
        // You obviously don't need any of this for native platforms
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows)) return process.Id;

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

        if (OperatingSystem.IsLinux() && ResolveRuntime() == WineRuntimeMode.Proton)
        {
            return Path.GetFullPath(TempestPathUtility.GetProtonDataDirectory());
        }

        return Path.GetFullPath(TempestPathUtility.GetWinePrefixDirectory());
    }

    public static Task WaitForPrefix()
    {
        var process = new Process();

        process.StartInfo.FileName = "cmd.exe";
        process.StartInfo.Arguments = "/c echo DONE";

        process.UseWineBinary().Start();

        return process.WaitForExitAsync();
    }

    public static async Task<int> GetProcessId(string processName)
    {
        var process = new Process();

        process.StartInfo.FileName = "tasklist.exe";
        process.StartInfo.Arguments = "/fo csv";
        process.StartInfo.RedirectStandardOutput = true;
        process.StartInfo.CreateNoWindow = true;

        process.UseWineBinary().Start();

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

        // wmic is gone in modern Wine; fall back to the highest PID (usually the newest process).
        return newestTime == null ? pids.Max() : newestPid;
    }

    public static async Task<bool> IsWinePidAlive(int pid)
    {
        var process = new Process();

        process.StartInfo.FileName = "tasklist.exe";
        process.StartInfo.Arguments = $"/FI \"PID eq {pid}\" /fo csv";
        process.StartInfo.RedirectStandardOutput = true;
        process.StartInfo.CreateNoWindow = true;

        process.UseWineBinary().Start();

        string output = await process.StandardOutput.ReadToEndAsync();
        await process.WaitForExitAsync();

        return output.Contains($"\"{pid}\"");
    }

    public static string? ResolveProtonDirectory()
    {
        var env = Environment.GetEnvironmentVariable("PROTON");
        if (!string.IsNullOrWhiteSpace(env))
        {
            if (env.Equals("wine", StringComparison.OrdinalIgnoreCase) ||
                env.Equals("false", StringComparison.OrdinalIgnoreCase) ||
                env.Equals("off", StringComparison.OrdinalIgnoreCase) ||
                env.Equals("system", StringComparison.OrdinalIgnoreCase))
            {
                return null;
            }

            var dir = ExpandTilde(env);
            if (IsProtonDirectory(dir))
            {
                return dir;
            }

            Console.Error.WriteLine($"Warning: PROTON environment variable points to '{env}', which doesn't look like a Proton installation. Falling back to system Wine.");
            return null;
        }

        return DetectProtonDirectoryCached();
    }

    private static string? _detectedProtonDir;
    private static bool _detectedProton;
    private static bool _protonAnnounced;

    private static string? DetectProtonDirectoryCached()
    {
        if (!_detectedProton)
        {
            _detectedProton = true;
            _detectedProtonDir = DetectProtonDirectory();
            if (_detectedProtonDir != null && !_protonAnnounced)
            {
                _protonAnnounced = true;
                Console.Error.WriteLine($"Using Proton: {_detectedProtonDir} (set PROTON to a directory to override, or PROTON=wine to force system Wine)");
            }
        }
        return _detectedProtonDir;
    }

    /// <summary>
    /// The runtime in effect: the one chosen for the running game if any,
    /// otherwise the configured/auto-detected one.
    /// </summary>
    private static WineRuntimeMode ResolveRuntime()
    {
        if (_gameRuntime != WineRuntimeMode.Auto) return _gameRuntime;
        return ResolveProtonDirectory() != null ? WineRuntimeMode.Proton : WineRuntimeMode.SystemWine;
    }

    /// <summary>Auto-detects Proton installations, best first (used by the launcher UI and the Auto fallback).</summary>
    public static IEnumerable<string> EnumerateProtonDirectories()
    {
        if (!OperatingSystem.IsLinux()) yield break;

        var candidates = new List<string>();
        var seenRoots = new HashSet<string>(StringComparer.Ordinal);

        // Steam (and Flatpak Steam) installs. Roots such as ~/.steam/root are often symlinks to
        // the same install; resolve and dedupe them so each Proton shows up exactly once.
        foreach (var root in SteamInstallRoots())
        {
            var real = ResolveRoot(root);
            if (!seenRoots.Add(real)) continue;

            var compatTools = Path.Combine(real, "compatibilitytools.d");
            if (Directory.Exists(compatTools))
            {
                foreach (var dir in Directory.GetDirectories(compatTools))
                {
                    if (IsProtonDirectory(dir)) candidates.Add(dir);
                }
            }

            // Valve's built-in Proton installs live under steamapps/common, not compatibilitytools.d.
            var common = Path.Combine(real, "steamapps", "common");
            if (Directory.Exists(common))
            {
                foreach (var dir in Directory.GetDirectories(common))
                {
                    if (dir.Contains("Proton", StringComparison.OrdinalIgnoreCase) && IsProtonDirectory(dir))
                    {
                        candidates.Add(dir);
                    }
                }
            }
        }

        // System-wide installs (rare).
        foreach (var root in new[] { "/usr/share/steam", "/usr/local/share/steam" })
        {
            var compatTools = Path.Combine(root, "compatibilitytools.d");
            if (!Directory.Exists(compatTools)) continue;
            foreach (var dir in Directory.GetDirectories(compatTools))
            {
                if (IsProtonDirectory(dir)) candidates.Add(dir);
            }
        }

        foreach (var candidate in candidates.OrderByDescending(GetProtonVersionKey))
        {
            yield return candidate;
        }
    }

    private static IEnumerable<string> SteamInstallRoots()
    {
        foreach (var root in new[]
        {
            "~/.steam/root",
            "~/.steam/steam",
            "~/.local/share/Steam",
            "~/.var/app/com.valvesoftware.Steam/data/Steam",
        })
        {
            var full = ExpandTilde(root);
            if (Directory.Exists(full)) yield return full;
        }
    }

    /// <summary>Resolves a final-component symlink (e.g. ~/.steam/root) using the managed API.</summary>
    private static string ResolveRoot(string path)
    {
        try
        {
            return new DirectoryInfo(path).ResolveLinkTarget(true)?.FullName ?? Path.GetFullPath(path);
        }
        catch
        {
            return Path.GetFullPath(path);
        }
    }

    public static string? DetectProtonDirectory() => EnumerateProtonDirectories().FirstOrDefault();

    private static string GetProtonVersionKey(string dir)
    {
        var versionFile = Path.Combine(dir, "version");
        return File.Exists(versionFile) ? File.ReadAllText(versionFile).Trim() : dir;
    }

    private static bool IsProtonDirectory(string dir)
    {
        if (!Directory.Exists(dir)) return false;
        var proton = Path.Combine(dir, "proton");
        return File.Exists(proton);
    }

    private static string? GetProtonWineBinary(string protonDir)
    {
        foreach (var relative in new[] { "files/bin/wine64", "files/bin/wine" })
        {
            var candidate = Path.Combine(protonDir, relative);
            if (File.Exists(candidate)) return candidate;
        }
        return null;
    }

    /// <summary>True when the Steam client is running (its IPC socket dir exists).
    /// Needed for Steam AppId registration / rich presence.</summary>
    private static bool IsSteamRunning()
    {
        try
        {
            return Directory.GetDirectories("/tmp")
                .Any(d => d.Contains(".com.valvesoftware.Steam", StringComparison.OrdinalIgnoreCase));
        }
        catch
        {
            return false;
        }
    }

    /// <summary>
    /// Proton's protonfixes extracts a game id from digits in STEAM_COMPAT_DATA_PATH; without any
    /// digits the script crashes. Ensure the path always ends with the game's Steam AppId.
    /// </summary>
    private static string EnsureProtonCompatDataPath(string dataPath)
    {
        if (dataPath.Any(char.IsDigit)) return dataPath;
        return Path.Combine(dataPath, TempestPathUtility.SteamAppId.ToString());
    }

    private static bool IsSystemWineAvailable()
    {
        if (!string.IsNullOrWhiteSpace(Environment.GetEnvironmentVariable("WINE"))) return true;
        return FindOnPath("wine") != null;
    }

    private static string? FindOnPath(string name)
    {
        var path = Environment.GetEnvironmentVariable("PATH");
        if (string.IsNullOrEmpty(path)) return null;
        foreach (var dir in path.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries))
        {
            var candidate = Path.Combine(dir, name);
            if (File.Exists(candidate)) return candidate;
        }
        return null;
    }

    /// <summary>Returns the Steam install root if one exists (used for STEAM_COMPAT_CLIENT_INSTALL_PATH and SLR lookups).</summary>
    private static string? GetSteamRoot()
    {
        if (!OperatingSystem.IsLinux()) return null;

        var candidates = new[]
        {
            "~/.steam/root",
            "~/.steam/steam",
            "~/.local/share/Steam",
            "~/.var/app/com.valvesoftware.Steam/data/Steam",
        };

        foreach (var candidate in candidates)
        {
            var full = ExpandTilde(candidate);
            if (Directory.Exists(full) && File.Exists(Path.Combine(full, "steamapps", "libraryfolders.vdf")))
            {
                return full;
            }
        }

        return null;
    }

    /// <summary>Finds Steam Linux Runtime "sniper" across all Steam library folders (from libraryfolders.vdf).</summary>
    private static string? ResolveSteamLinuxRuntime()
    {
        if (!OperatingSystem.IsLinux()) return null;

        var roots = new List<string>();
        if (GetSteamRoot() is { } steamRoot)
        {
            roots.Add(steamRoot);
            roots.AddRange(ReadLibraryFolders(Path.Combine(steamRoot, "steamapps", "libraryfolders.vdf")));
        }
        // Flatpak Steam keeps its library file elsewhere
        var flatpak = ExpandTilde("~/.var/app/com.valvesoftware.Steam/data/Steam");
        if (Directory.Exists(flatpak) && !roots.Contains(flatpak))
        {
            roots.Add(flatpak);
            roots.AddRange(ReadLibraryFolders(Path.Combine(flatpak, "steamapps", "libraryfolders.vdf")));
        }

        foreach (var root in roots)
        {
            var run = Path.Combine(root, "steamapps", "common", "SteamLinuxRuntime_sniper", "run");
            if (File.Exists(run) && IsExecutable(run))
            {
                return run;
            }
        }

        return null;
    }

    /// <summary>Parses steamapps/libraryfolders.vdf without regex (AOT-safe): collects library paths.</summary>
    private static IEnumerable<string> ReadLibraryFolders(string vdfPath)
    {
        if (!File.Exists(vdfPath)) yield break;
        foreach (var line in File.ReadAllLines(vdfPath))
        {
            var trimmed = line.Trim();
            if (!trimmed.StartsWith("\"path\"")) continue;
            var first = trimmed.IndexOf('"', 7);
            var last = trimmed.LastIndexOf('"');
            if (first < 0 || last <= first) continue;
            var path = trimmed.Substring(first + 1, last - first - 1);
            if (path.Length > 0) yield return path;
        }
    }

    /// <summary>
    /// Steam Linux Runtime must be new enough to run the (modern) Proton script: it requires
    /// Python 3.11+ (typing.Self). Probe the runtime's own python directly and cache the result.
    /// </summary>
    private static string? _steamRuntimeCompatKey;
    private static bool _steamRuntimeCompatible;

    private static bool IsSteamRuntimeCompatible(string slrRun)
    {
        var dir = Path.GetDirectoryName(slrRun);
        if (dir == null) return false;

        if (_steamRuntimeCompatKey == dir) return _steamRuntimeCompatible;

        _steamRuntimeCompatKey = dir;
        _steamRuntimeCompatible = false;

        try
        {
            // The runtime's own python binary (host-arch) can be executed directly, no container spawn needed.
            var python = Directory.GetDirectories(Path.Combine(dir, "var"))
                .Select(tmp => Path.Combine(tmp, "usr", "bin", "python3"))
                .FirstOrDefault(File.Exists);

            if (python == null) return false;

            var psi = new ProcessStartInfo
            {
                FileName = python,
                Arguments = "--version",
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true,
            };
            using var proc = Process.Start(psi);
            var output = proc!.StandardError.ReadToEnd() + proc.StandardOutput.ReadToEnd();
            proc.WaitForExit(5000);
            if (proc.HasExited && proc.ExitCode != 0) return false;

            // output like "Python 3.9.2" or "Python 3.11.8"
            var match = System.Text.RegularExpressions.Regex.Match(output, @"Python (\d+)\.(\d+)");
            if (match.Success)
            {
                int.TryParse(match.Groups[1].Value, out var major);
                int.TryParse(match.Groups[2].Value, out var minor);
                var compatible = major > 3 || (major == 3 && minor >= 11);
                _steamRuntimeCompatible = compatible;
                return compatible;
            }
        }
        catch
        {
            // ignore
        }

        return false;
    }

    private static bool IsExecutable(string path)
    {
        if (!OperatingSystem.IsLinux()) return false;
        try
        {
            var mode = File.GetUnixFileMode(path);
            return (mode & UnixFileMode.UserExecute) != 0;
        }
        catch
        {
            return false;
        }
    }

    private static string ExpandTilde(string path)
    {
        if (path == "~") return Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        return path.StartsWith("~/", StringComparison.Ordinal)
            ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), path[2..])
            : path;
    }

    private static void SetCommonWineEnvironment(Process process)
    {
        // disable all kinds of logs
        process.StartInfo.EnvironmentVariables["DXVK_LOG_LEVEL"] = "none";
        process.StartInfo.EnvironmentVariables["DXVK_LOG_PATH"] = "/dev/null";
        process.StartInfo.EnvironmentVariables["WINEDEBUG"] = "-all";
        process.StartInfo.EnvironmentVariables["NATIVE_FILENAME"] = process.StartInfo.FileName;
    }

    private static void InsertExecutableFirst(Process process, string filename)
    {
        if (process.StartInfo.ArgumentList.Count > 0)
        {
            process.StartInfo.ArgumentList.Insert(0, filename);
        }
        else
        {
            process.StartInfo.Arguments = $"\"{filename.Replace("\"", "\\\"")}\" {process.StartInfo.Arguments}";
        }
    }

    private static void InsertArgumentFirst(Process process, string arg)
    {
        if (process.StartInfo.ArgumentList.Count > 0)
        {
            process.StartInfo.ArgumentList.Insert(0, arg);
        }
        else
        {
            process.StartInfo.Arguments = $"{arg} {process.StartInfo.Arguments}";
        }
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

            process.UseWineBinary().Start();

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
            // wmic is deprecated/removed in modern Wine; the caller falls back to max PID.
            return null;
        }

        return null;
    }
}