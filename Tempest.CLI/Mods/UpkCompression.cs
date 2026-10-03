using System.Diagnostics;
using Tempest.CLI.Extensions;

namespace Tempest.CLI.Mods;

internal static class UpkCompression
{
    internal static async Task CompressAsync(string input, string output, string tool)
    {
        input = Path.GetFullPath(input);
        output = Path.GetFullPath(output);
        tool = Path.GetFullPath(tool);
        if (!File.Exists(input)) throw new FileNotFoundException("UPK file not found", input);
        if (!File.Exists(tool)) throw new FileNotFoundException("UPK compression tool not found", tool);
        if (!Path.GetExtension(input).Equals(".upk", StringComparison.OrdinalIgnoreCase))
            throw new ArgumentException("Only UPK files can be compressed.");
        if (input.Equals(output, OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal))
            throw new ArgumentException("Compression output must be separate from the source file.");
        if (File.Exists(output)) throw new IOException("Compression output already exists.");

        var start = new ProcessStartInfo(tool)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
        };
        start.ArgumentList.Add(OperatingSystem.IsWindows() ? input : "Z:" + input.Replace('/', '\\'));
        start.ArgumentList.Add("-save");
        start.ArgumentList.Add("compressed");
        start.ArgumentList.Add(OperatingSystem.IsWindows() ? output : "Z:" + output.Replace('/', '\\'));
        using var process = new Process { StartInfo = start };
        process.UseWineBinary();
        if (!process.Start()) throw new IOException("Could not start the compression tool.");
        var stdout = process.StandardOutput.ReadToEndAsync();
        var stderr = process.StandardError.ReadToEndAsync();
        using var timeout = new CancellationTokenSource(TimeSpan.FromMinutes(5));
        try { await process.WaitForExitAsync(timeout.Token); }
        catch (OperationCanceledException)
        {
            process.Kill(entireProcessTree: true);
            await process.WaitForExitAsync();
            throw new TimeoutException("UPK compression timed out.");
        }
        await stdout;
        var error = await stderr;
        if (process.ExitCode != 0)
            throw new IOException($"UPK compression failed (exit {process.ExitCode}): {error.Trim()}");
        await using var result = File.OpenRead(output);
        var tag = new byte[4];
        await result.ReadExactlyAsync(tag);
        if (BitConverter.ToUInt32(tag) != 0x9E2A83C1 || result.Length < 32)
            throw new IOException("Compression tool did not produce a valid UPK package.");
    }
}
