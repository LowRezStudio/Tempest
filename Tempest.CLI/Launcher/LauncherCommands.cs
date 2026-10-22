using System.Diagnostics;
using ConsoleAppFramework;
using Tempest.CLI.Extensions;
using Tempest.CLI.Mods;

namespace Tempest.CLI.Launcher;

internal class LauncherCommands
{
    public async Task Launch([Argument] string path, ConsoleAppContext context, bool noDefaultArgs = false, string? platform = null, string? game = null, string[]? dll = null, string? homedir = null, bool gamescope = false, string? gamescopeArgs = "-f --force-grab-cursor", bool steamRuntime = false, bool enableOpenSslFix = true, bool noDlls = false)
    {
        var args = context.EscapedArguments.ToArray();
        var process = await LaunchGame(path, args, noDefaultArgs, platform, game, dll, false, homedir, gamescope, gamescopeArgs, steamRuntime, enableOpenSslFix, noDlls);

        // Proton (and gamescope) wrappers can outlive the game itself. The launcher only marks
        // the game as stopped when this process exits, so on Linux watch the real game process and
        // tear the wine tree down as soon as it disappears instead of waiting on the wrapper.
        if (!OperatingSystem.IsWindows())
        {
            var env = process.StartInfo.EnvironmentVariables;
            var nativePath = env.ContainsKey("NATIVE_FILENAME") ? env["NATIVE_FILENAME"] : null;
            var nativeFilename = Path.GetFileName(nativePath);
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
                    var wrapperExited = process.WaitForExitAsync();
                    while (true)
                    {
                        await Task.WhenAny(wrapperExited, Task.Delay(TimeSpan.FromSeconds(1)));
                        if (!await WineExtensions.IsWinePidAlive(gamePid))
                        {
                            await WineExtensions.KillProcessTree(process, sweepPrefix: false);
                            break;
                        }
                        if (process.HasExited)
                        {
                            // Stopped from the launcher. Proton's steam.exe starts the game with CreateProcess,
                            // which Wine forks out of the wrapper's process tree, so killing the wrapper missed it.
                            await WineExtensions.KillWinePid(gamePid);
                            break;
                        }
                    }
                }
            }
        }

        await process.WaitForExitAsync();

        // The game is gone. A Wine prefix can host other windows of the same instance, so only
        // stop this launch's process tree instead of sweeping the whole prefix.
        await WineExtensions.KillProcessTree(process, sweepPrefix: false);
    }
public static async Task<Process> LaunchGame(string path, string[] args, bool noDefaultArgs = false,
                                                 string? platform = null, string? game = null, string[]? dll = null,
                                                 bool isServer = false, string? homedir = null, bool gamescope = false, string? gamescopeArgs = "-f --force-grab-cursor", bool steamRuntime = false, bool enableOpenSslFix = true, bool noDlls = false)
    {
        if (isServer) gamescope = false;

        // Before the game reads any mod file: mods signed with a since-revoked key are switched off,
        // and mods whose files were changed after install are re-verified against their signed CHECKSUMS.
        await ModCommands.DisableRevokedModsAsync(path);
        await ModIntegrity.VerifyAtLaunchAsync(path);
        ModDependencies.WarnAtLaunch(path);

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
        }

        var process = new Process();

        process.StartInfo.FileName = exePath;
        if (enableOpenSslFix)
        {
            process.StartInfo.Environment["OPENSSL_ia32cap"] = "~0x20000000"; // Fix for the 64bit clients not working on 10th Gen and 11th Gen Intel CPUs
        }

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

                // Kill the wrapper; the launch path stops its process tree once it exits.
                try
                {
                    process.Kill(true);
                }
                catch
                {
                    // already gone
                }
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
                    if (mod.Enabled && mod.Verified && string.Equals(mod.Kind, "V2", StringComparison.OrdinalIgnoreCase))
                    {
                        // dlls/64 and dlls/32 are per client; dlls/ itself is the legacy layout.
                        var dllsDir = Path.Combine(TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, mod.Id), "dlls");
                        var dlls = ModDlls.ForProcess(dllsDir, is64Bit);
                        if (dlls.Count > 0 && mod.Signer.Length == 0)
                        {
                            Console.Error.WriteLine($"[inject] skipping the DLLs of '{mod.Name}': it was installed unsigned; reinstall a signed build");
                            continue;
                        }
                        allDlls.AddRange(dlls);
                    }
                }
            }
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Warning: Failed to load V2 mod DLLs for injection: {ex.Message}");
        }

        // `--dll` paths are taken as given, except that a DLL built for the other
        // client cannot load into this process and is skipped instead.
        var matchingDlls = allDlls.Distinct(StringComparer.OrdinalIgnoreCase).Where(d =>
        {
            var dll64 = PeImage.Is64Bit(d);
            if (dll64 is null || dll64 == is64Bit)
                return true;
            Console.Error.WriteLine($"[inject] skipping {Path.GetFileName(d)}: built for {(dll64.Value ? "Win64" : "Win32")}, the game is {(is64Bit ? "Win64" : "Win32")}");
            return false;
        }).ToList();
        if (matchingDlls.Count > 0 && noDlls)
        {
            Console.Error.WriteLine($"[inject] --no-dlls: not injecting {matchingDlls.Count} DLL(s)");
        }
        else if (matchingDlls.Count > 0)
        {
            await Task.WhenAll(matchingDlls.Select(d => process.InjectLibraryAsync(d, is64Bit)));
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
