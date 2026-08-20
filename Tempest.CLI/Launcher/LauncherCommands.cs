using System.Diagnostics;
using ConsoleAppFramework;
using Tempest.CLI.Extensions;
using Tempest.CLI.Mods;

namespace Tempest.CLI.Launcher;

internal class LauncherCommands
{
    public async Task Launch([Argument] string path, ConsoleAppContext context, bool noDefaultArgs = false, string? platform = null, string? game = null, string[]? dll = null, string? homedir = null, bool gamescope = false, string? gamescopeArgs = "-f --force-grab-cursor", bool steamRuntime = false)
    {
        var args = context.EscapedArguments.ToArray();
        var process = await LaunchGame(path, args, noDefaultArgs, platform, game, dll, false, homedir, gamescope, gamescopeArgs, steamRuntime);

        if (gamescope && !OperatingSystem.IsWindows())
        {
            var nativeFilename = Path.GetFileName(process.StartInfo.EnvironmentVariables["NATIVE_FILENAME"]);
            if (!string.IsNullOrEmpty(nativeFilename))
            {
                int gamePid = 0;
                // Wait for the game process to start and obtain its PID
                for (int i = 0; i < 15 && !process.HasExited; i++)
                {
                    gamePid = await WineExtensions.GetProcessId(nativeFilename);
                    if (gamePid > 0) break;
                    await Task.Delay(TimeSpan.FromSeconds(1));
                }

                if (gamePid > 0)
                {
                    while (!process.HasExited)
                    {
                        await Task.Delay(TimeSpan.FromSeconds(1));
                        if (!await WineExtensions.IsWinePidAlive(gamePid))
                        {
                            await WineExtensions.KillProcessTree(process);
                            break;
                        }
                    }
                }
            }
        }

        await process.WaitForExitAsync();

        // If a stop was requested via stdin, the reader thread performs a /proc sweep of the wine
        // tree; the wrapper may exit before that sweep finishes, so give it a moment to complete.
        for (var i = 0; WineExtensions.KillInProgress && i < 200; i++)
        {
            await Task.Delay(25);
        }

        // The game is gone. Sweep any wine/proton processes (wineserver, CoherentUI renderers,
        // the lsteamclient bridge, winedevice) that can outlive the wrapper, so nothing is left
        // behind after the game stops or its window is closed.
        await WineExtensions.KillProcessTree(process);
    }
    public static async Task<Process> LaunchGame(string path, string[] args, bool noDefaultArgs = false,
                                                string? platform = null, string? game = null, string[]? dll = null,
                                                bool isServer = false, string? homedir = null, bool gamescope = false, string? gamescopeArgs = "-f --force-grab-cursor", bool steamRuntime = false)
    {
        if (isServer) gamescope = false;

        var exePath = LauncherUtility.GetExecutablePath(path, platform, game);
        var defaultArgs = !noDefaultArgs;
        var is64Bit = Directory.GetParent(exePath)?.Name == "Win64";

        // Rename EasyAntiCheat folders to prevent crash when loading anti-cheat with Wine
        if (!OperatingSystem.IsWindows())
        {
            var binariesDir = Directory.GetParent(exePath)?.Parent?.FullName;
            if (binariesDir != null)
            {
                var eacPath = Path.Combine(binariesDir, "EasyAntiCheat");
                var eacPathRenamed = Path.Combine(binariesDir, "_EasyAntiCheat");
                if (Directory.Exists(eacPath) && !Directory.Exists(eacPathRenamed))
                {
                    Directory.Move(eacPath, eacPathRenamed);
                }
            }

            var platformDir = Directory.GetParent(exePath)?.FullName;
            if (platformDir != null)
            {
                var platformEacPath = Path.Combine(platformDir, "EasyAntiCheat");
                var platformEacPathRenamed = Path.Combine(platformDir, "_EasyAntiCheat");
                if (Directory.Exists(platformEacPath) && !Directory.Exists(platformEacPathRenamed))
                {
                    Directory.Move(platformEacPath, platformEacPathRenamed);
                }
            }
        }

        var process = new Process();

        process.StartInfo.FileName = exePath;
        process.StartInfo.Environment["OPENSSL_ia32cap"] = "~0x20000000"; // Fix for the 64bit clients not working on 10th Gen and 11th Gen Intel CPUs

        foreach (var arg in args)
        {
            process.StartInfo.ArgumentList.Add(arg);
        }

        if (defaultArgs)
        {
            process.StartInfo.ArgumentList.Add("-seekfreeloadingpcconsole");
            process.StartInfo.ArgumentList.Add("-pid=402");
            process.StartInfo.ArgumentList.Add("-eac-nop-loaded");
            process.StartInfo.ArgumentList.Add("-replayfile=");
            process.StartInfo.ArgumentList.Add("-COOKFORDEMO");
            if (gamescope)
            {
                process.StartInfo.ArgumentList.Add("-nosplash");
                process.StartInfo.ArgumentList.Add("-windowed");
            }
            homedir ??= "Tempest";
            process.StartInfo.ArgumentList.Add($"-homedir={homedir}");
        }

        if (gamescope)
        {
            process.UseWine(isServer, !isServer && steamRuntime).UseGamescope(gamescopeArgs).Start();
        }
        else
        {
            process.UseWine(isServer, !isServer && steamRuntime).Start();
        }

        _ = Task.Run(async () =>
        {
            using var reader = new StreamReader(Console.OpenStandardInput());
            while (!process.HasExited)
            {
                var input = await reader.ReadLineAsync();

                if (input == null || !input.Trim().Equals("kill", StringComparison.OrdinalIgnoreCase)) continue;

                try
                {
                    // Bound the sweep so the CLI always exits even if /proc reads misbehave;
                    // the launcher relies on this process exiting to mark the game as stopped.
                    await Task.WhenAny(WineExtensions.KillProcessTree(process), Task.Delay(TimeSpan.FromSeconds(15)));
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine($"Failed to stop the game: {ex.Message}");
                }
                Environment.Exit(0);
                break;
            }
        });

        await Task.Delay(TimeSpan.FromSeconds(1));

        var allDlls = new List<string>();
        if (dll != null)
        {
            allDlls.AddRange(dll);
        }

        try
        {
            var resolvedGame = GameFolderResolver.Resolve(path);
            var metadataPath = ModCommands.GetMetadataPath(path);
            if (File.Exists(metadataPath))
            {
                var mods = ModCommands.LoadMetadata(path);
                foreach (var mod in mods)
                {
                    if (mod.Enabled && string.Equals(mod.Kind, "V2", StringComparison.OrdinalIgnoreCase))
                    {
                        var dllsDir = Path.Combine(TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, mod.Id), "dlls");
                        if (Directory.Exists(dllsDir))
                        {
                            var files = Directory.GetFiles(dllsDir, "*.dll", SearchOption.AllDirectories);
                            allDlls.AddRange(files);
                        }
                    }
                }
            }
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Warning: Failed to load V2 mod DLLs for injection: {ex.Message}");
        }

        if (allDlls.Count > 0)
        {
            await Task.WhenAll(allDlls.Distinct(StringComparer.OrdinalIgnoreCase).Select(d => process.InjectLibraryAsync(d, is64Bit)));
        }
        if (isServer)
        {
#if DEBUG
            var asmLoaderPath = Path.Combine(AppContext.BaseDirectory, $"../../../../Tempest.Utils/zig-out/bin/asmloader-windows_x86{(is64Bit ? "_64" : "")}.dll");
#else
            var asmLoaderPath = Path.Combine(AppContext.BaseDirectory, $"asmloader-windows_x86{(is64Bit ? "_64" : "")}.dll");
#endif
            await process.InjectLibraryAsync(asmLoaderPath, is64Bit);
        }

        return process;
    }
}
