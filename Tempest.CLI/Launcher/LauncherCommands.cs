using System.Diagnostics;
using ConsoleAppFramework;
using Tempest.CLI.Extensions;

namespace Tempest.CLI.Launcher;

internal class LauncherCommands
{
    public async Task Launch([Argument] string path, ConsoleAppContext context, bool noDefaultArgs = false, string? platform = null, string? game = null, string[]? dll = null)
    {
        var args = context.EscapedArguments.ToArray();
        var process = await LaunchGame(path, args, noDefaultArgs, platform, game, dll);
        await process.WaitForExitAsync();
    }
    public static async Task<Process> LaunchGame(string path, string[] args, bool noDefaultArgs = false,
                                                string? platform = null, string? game = null, string[]? dll = null,
                                                bool isServer = false)
    {
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
        process.StartInfo.Environment["OPENSSL_ia32cap"] = "~0x200000200000000"; // Fix for the 64bit clients not working on 10th Gen and 11th Gen Intel CPUs

        foreach (var arg in args)
        {
            process.StartInfo.ArgumentList.Add(arg);
        }

        if (defaultArgs)
        {
            process.StartInfo.ArgumentList.Add("-seekfreeloadingpcconsole");
            process.StartInfo.ArgumentList.Add("-pid=402");
            process.StartInfo.ArgumentList.Add("-anon");
            process.StartInfo.ArgumentList.Add("-nosteam");
            process.StartInfo.ArgumentList.Add("-eac-nop-loaded");
            process.StartInfo.ArgumentList.Add("-replayfile=");
            process.StartInfo.ArgumentList.Add("-COOKFORDEMO");
            process.StartInfo.ArgumentList.Add("-homedir=Tempest");
        }

        process.UseWine().Start();

        _ = Task.Run(async () =>
        {
            using var reader = new StreamReader(Console.OpenStandardInput());
            while (!process.HasExited)
            {
                var input = await reader.ReadLineAsync();

                if (input == null || !input.Trim().Equals("kill", StringComparison.OrdinalIgnoreCase)) continue;

                process.Kill(true);
                Environment.Exit(0);
                break;
            }
        });

        await Task.Delay(TimeSpan.FromSeconds(1));

        if (dll != null)
        {
            await Task.WhenAll(dll.Select(d => process.InjectLibraryAsync(d, is64Bit)));
        }
        if (isServer)
        {
            //TODO fix, obviously now only works in dev environment.
            var asmLoaderPath = $"{Directory.GetCurrentDirectory()}/../Tempest.Utils/zig-out/bin/asmloader-windows_x86{(is64Bit ? "_64" : "")}.dll";
            await process.InjectLibraryAsync(asmLoaderPath, is64Bit);
        }

        return process;
    }
}
