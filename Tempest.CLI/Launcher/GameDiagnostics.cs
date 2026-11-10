using System.ComponentModel;
using System.Diagnostics;
using System.Security.Cryptography;
using System.Text.Json.Nodes;
using Tempest.CLI.Mods;
using Tomlyn;
using Tomlyn.Model;

namespace Tempest.CLI.Launcher;

internal sealed class GameDiagnostics(string? outputDirectory = null) : IAsyncDisposable
{
    public string SnapshotPath { get; } = Path.Combine(outputDirectory ?? Path.Combine(Path.GetTempPath(), "TempestCrashDiagnostics"), $"{Guid.NewGuid():N}.json");
    public string DevModsPath { get; private set; } = "";
    private Task? observer;
    private readonly CancellationTokenSource stop = new();

    public void Start(Process game, string gamePath)
    {
        try
        {
            DevModsPath = ResolveDevModsPath(game, GameFolderResolver.Resolve(gamePath));
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or TomlException)
        {
            Console.Error.WriteLine($"[diagnostics] Could not resolve dev-mod configuration: {error.Message}");
        }
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(SnapshotPath)!);
            foreach (var old in Directory.EnumerateFiles(Path.GetDirectoryName(SnapshotPath)!, "*.json*"))
            {
                var name = Path.GetFileName(old);
                try
                {
                    if (Guid.TryParseExact(name.Split('.')[0], "N", out _) &&
                        (name.EndsWith(".json") || name.EndsWith(".json.next") || name.EndsWith(".json.bak")) &&
                        File.GetLastWriteTimeUtc(old) < DateTime.UtcNow.AddDays(-2))
                        File.Delete(old);
                }
                catch (Exception error) when (error is IOException or UnauthorizedAccessException)
                {
                    Console.Error.WriteLine($"[diagnostics] Could not prune an old module snapshot: {error.Message}");
                }
            }
            var pid = game.Id;
            observer = Task.Run(() => Observe(pid, stop.Token));
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or Win32Exception)
        {
            Console.Error.WriteLine($"[diagnostics] Could not start crash diagnostics: {error.Message}");
        }
    }

    private static string ResolveDevModsPath(Process game, string root)
    {
        var config = game.StartInfo.Environment.TryGetValue("TEMPEST_SCRIPT_CONFIG", out var configured) && !string.IsNullOrEmpty(configured)
            ? configured : Path.Combine(root, "ChaosGame", "Config", "tempest.toml");
        var mods = "mods";
        if (File.Exists(config))
        {
            var table = TomlSerializer.Deserialize<TomlTable>(File.ReadAllText(config), ModTomlContext.Default.TomlTable);
            if (table?.TryGetValue("dev", out var dev) == true && dev is TomlTable settings &&
                settings.TryGetValue("mods", out var value) && value is string folder)
                mods = folder;
        }
        return Path.GetFullPath(Path.IsPathRooted(mods) ? mods : Path.Combine(root, mods));
    }

    private async Task Observe(int pid, CancellationToken token)
    {
        var modules = new JsonArray();
        var errors = new JsonArray();
        var complete = false;
        try
        {
            if (!OperatingSystem.IsWindows())
            {
                errors.Add((JsonNode)"Loaded-module collection is only supported on Windows.");
                complete = true;
                return;
            }
            using var game = Process.GetProcessById(pid);
            var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            while (!game.HasExited)
            {
                game.Refresh();
                ProcessModuleCollection loaded;
                try { loaded = game.Modules; }
                catch (Exception error) when (error is Win32Exception or InvalidOperationException && game.HasExited) { break; }
                foreach (ProcessModule module in loaded)
                {
                    var file = module.FileName;
                    if (!seen.Add(file)) continue;
                    var record = new JsonObject { ["path"] = file, ["sha256"] = null };
                    modules.Add((JsonNode)record);
                    try
                    {
                        using var stream = File.OpenRead(file);
                        record["sha256"] = Convert.ToHexStringLower(await SHA256.HashDataAsync(stream, token));
                    }
                    catch (Exception error) when (error is IOException or UnauthorizedAccessException) { record["error"] = error.Message; }
                }
                await Task.Delay(1000, token);
            }
            complete = true;
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested)
        {
            errors.Add((JsonNode)"Module collection timed out; this snapshot is incomplete.");
        }
        catch (Exception error) when (error is Win32Exception or InvalidOperationException or ArgumentException)
        {
            errors.Add((JsonNode)error.Message);
        }
        finally
        {
            var snapshot = new JsonObject { ["modules"] = modules, ["errors"] = errors, ["complete"] = complete };
            await File.WriteAllTextAsync(SnapshotPath, snapshot.ToJsonString());
        }
    }

    public async ValueTask DisposeAsync()
    {
        try
        {
            if (observer == null) return;
            try { await observer.WaitAsync(TimeSpan.FromSeconds(10)); }
            catch (TimeoutException) { await stop.CancelAsync(); await observer; }
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            Console.Error.WriteLine($"[diagnostics] Could not save module snapshot: {error.Message}");
        }
        finally { stop.Dispose(); }
    }
}
