using System.Diagnostics;
using ConsoleAppFramework;
using Tempest.CLI.Extensions;

namespace Tempest.CLI.Launcher;

internal class LauncherCommands
{
    public async Task Launch([Argument] string path, ConsoleAppContext context, bool noDefaultArgs = false, string? platform = null, string? game = null, string[]? dll = null)
    {
        var exePath = LauncherUtility.GetExecutablePath(path, platform, game);
        var defaultArgs = !noDefaultArgs;
        var is64Bit = Directory.GetParent(exePath)?.Name == "Win64";

        var process = new Process();

        process.StartInfo.FileName = exePath;
        process.StartInfo.Environment["OPENSSL_ia32cap"] = "~0x200000200000000"; // Fix for the 64bit clients not working on 10th Gen and 11th Gen Intel CPUs

        foreach (var arg in context.EscapedArguments)
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

        await Task.Delay(TimeSpan.FromSeconds(1));

        if (dll != null)
        {
            await Task.WhenAll(dll.Select(d => process.InjectLibraryAsync(d, is64Bit)));
        }

        await process.WaitForExitAsync();
    }
}
