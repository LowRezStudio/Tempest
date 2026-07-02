using System.Diagnostics;
using System.Runtime.InteropServices;

namespace Tempest.CLI.Server;

internal static class FirewallHelper
{
    private static async Task<bool> RunCommandAsync(string fileName, string[] arguments, ILogger logger)
    {
        try
        {
            using var process = new Process();
            process.StartInfo.FileName = fileName;
            process.StartInfo.UseShellExecute = false;
            process.StartInfo.CreateNoWindow = true;
            process.StartInfo.RedirectStandardOutput = true;
            process.StartInfo.RedirectStandardError = true;

            foreach (var arg in arguments)
            {
                process.StartInfo.ArgumentList.Add(arg);
            }

            process.Start();

            var outputTask = process.StandardOutput.ReadToEndAsync();
            var errorTask = process.StandardError.ReadToEndAsync();

            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(10)); // Increased slightly for sudo prompts
            try
            {
                await process.WaitForExitAsync(cts.Token);
            }
            catch (OperationCanceledException)
            {
                process.Kill(true);
                logger.LogWarning("Command '{FileName} {Arguments}' timed out.", fileName, string.Join(" ", arguments));
                return false;
            }

            var output = await outputTask;
            var error = await errorTask;

            if (process.ExitCode != 0)
            {
                logger.LogDebug("Command '{FileName}' exited with code {ExitCode}. Output: {Output}, Error: {Error}",
                    fileName, process.ExitCode, output.Trim(), error.Trim());
                return false;
            }

            return true;
        }
        catch (Exception ex)
        {
            logger.LogDebug("Failed to run command '{FileName}': {Message}", fileName, ex.Message);
            return false;
        }
    }

    public static async Task OpenPortsAsync(int lobbyPort, int gamePort, ILogger logger)
    {
        logger.LogInformation("Attempting to open firewall ports: Lobby={LobbyPort} (TCP), Game={GamePort} (UDP)", lobbyPort, gamePort);

        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
        {
            // Windows: Using netsh to avoid PowerShell AV false-positives.
            // Note: netsh cannot create truly volatile rules. These rely on ClosePortsAsync for cleanup.
            string cmd = $"netsh advfirewall firewall add rule name=\"Tempest Lobby Port {lobbyPort}\" dir=in action=allow protocol=TCP localport={lobbyPort} && " +
                         $"netsh advfirewall firewall add rule name=\"Tempest Game Port {gamePort}\" dir=in action=allow protocol=UDP localport={gamePort}";

            if (await RunCommandAsync("cmd.exe", ["/c", cmd], logger))
                logger.LogInformation("Successfully opened firewall ports on Windows.");
            else
                logger.LogWarning("Failed to open firewall ports on Windows. Make sure you are running as Administrator.");
        }
        else if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux))
        {
            // Linux: skip if ports already open, use runtime-only configs (no --permanent flags)
            string checkAndOpen = $@"
                if command -v firewall-cmd >/dev/null 2>&1; then
                    if firewall-cmd --query-port={lobbyPort}/tcp >/dev/null 2>&1 && firewall-cmd --query-port={gamePort}/udp >/dev/null 2>&1; then
                        exit 0
                    fi
                    firewall-cmd --add-port={lobbyPort}/tcp && firewall-cmd --add-port={gamePort}/udp
                else
                    if iptables -C INPUT -p tcp --dport {lobbyPort} -j ACCEPT >/dev/null 2>&1 && iptables -C INPUT -p udp --dport {gamePort} -j ACCEPT >/dev/null 2>&1; then
                        exit 0
                    fi
                    iptables -I INPUT 1 -p tcp --dport {lobbyPort} -j ACCEPT && iptables -I INPUT 1 -p udp --dport {gamePort} -j ACCEPT
                fi";

            if (await ExecuteLinuxScriptAsync(checkAndOpen, logger))
                logger.LogInformation("Successfully opened temporary firewall ports on Linux.");
            else
                logger.LogWarning("Could not open temporary firewall ports on Linux. Please manually open {LobbyPort} (TCP) and {GamePort} (UDP).", lobbyPort, gamePort);
        }
    }

    public static async Task ClosePortsAsync(int lobbyPort, int gamePort, ILogger logger)
    {
        if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows)) return;

        logger.LogInformation("Attempting to close firewall ports: Lobby={LobbyPort} (TCP), Game={GamePort} (UDP)", lobbyPort, gamePort);

        string cmd = $"netsh advfirewall firewall delete rule name=\"Tempest Lobby Port {lobbyPort}\" & " +
                     $"netsh advfirewall firewall delete rule name=\"Tempest Game Port {gamePort}\"";

        await RunCommandAsync("cmd.exe", ["/c", cmd], logger);
    }

    private static Task<bool> ExecuteLinuxScriptAsync(string script, ILogger logger) =>
        RunCommandAsync("pkexec", ["sh", "-c", script], logger);
}
