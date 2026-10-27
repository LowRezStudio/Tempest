using System.Diagnostics;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Tempest.CLI.Mods;

/// <summary>
/// Game files that mods change through deltas (<c>deltas/&lt;game path&gt;.tdelta</c> package
/// entries: the objects or rows a mod added, changed or removed, see
/// <c>Tempest.UnrealScript/crates/tempest-assets/src/delta.rs</c>) are derived: the pristine
/// file (or the copy of the one mod that replaces it whole), with every enabled mod's delta
/// applied on top in install order, by the <c>upk</c> sidecar. The resolver rebuilds a file
/// whenever that stack changed (install, remove, enable, disable) and at launch, and restores
/// the backup when no mod contributes to it any more. <c>.tempest/v2/deltas.json</c> remembers
/// what each file was built from so an unchanged stack costs one hash.
/// </summary>
internal static class DeltaResolver
{
    private const string Prefix = "deltas/";
    private const string Suffix = ".tdelta";

    /// <summary>The game-relative path a <c>deltas/…</c> package entry applies to, or null when the entry is not one.</summary>
    public static string? TargetOf(string entryPath)
    {
        if (!entryPath.StartsWith(Prefix, StringComparison.OrdinalIgnoreCase) || !entryPath.EndsWith(Suffix, StringComparison.OrdinalIgnoreCase)) return null;
        var target = entryPath[Prefix.Length..^Suffix.Length].Replace('\\', '/');
        return target.Length == 0 ? null : target;
    }

    /// <summary>What a delta file touches: object paths of a package delta, <c>table:key</c> of a row delta (lower case). Used for conflicts between mods.</summary>
    public static List<string> Keys(Stream delta)
    {
        using var doc = ReadHeader(delta);
        var root = doc.RootElement;
        var keys = new List<string>();
        if (root.TryGetProperty("ops", out var ops) && ops.ValueKind == JsonValueKind.Array)
        {
            foreach (var op in ops.EnumerateArray())
                if (op.TryGetProperty("path", out var p) && p.ValueKind == JsonValueKind.String) keys.Add(p.GetString()!.ToLowerInvariant());
        }
        if (root.TryGetProperty("tables", out var tables) && tables.ValueKind == JsonValueKind.Array)
        {
            foreach (var table in tables.EnumerateArray())
            {
                var name = table.TryGetProperty("table", out var t) ? t.GetString() ?? "" : "";
                if (!table.TryGetProperty("ops", out var tops) || tops.ValueKind != JsonValueKind.Array) continue;
                foreach (var op in tops.EnumerateArray())
                    if (op.TryGetProperty("key", out var k)) keys.Add($"{name}:{k}".ToLowerInvariant());
            }
        }
        if (root.TryGetProperty("header", out var header) && header.ValueKind == JsonValueKind.Object)
        {
            foreach (var field in header.EnumerateObject()) keys.Add($"header:{field.Name}".ToLowerInvariant());
        }
        return keys;
    }

    /// <summary>Does the delta add script classes? Those packages are rebuilt first, so the others decode against the new classes.</summary>
    private static bool AddsClasses(string deltaFile)
    {
        try
        {
            using var stream = File.OpenRead(deltaFile);
            using var doc = ReadHeader(stream);
            if (!doc.RootElement.TryGetProperty("ops", out var ops) || ops.ValueKind != JsonValueKind.Array) return false;
            foreach (var op in ops.EnumerateArray())
            {
                if (op.TryGetProperty("op", out var o) && o.GetString() == "add" && op.TryGetProperty("class", out var c) && string.Equals(c.GetString(), "Class", StringComparison.OrdinalIgnoreCase))
                    return true;
            }
            return false;
        }
        catch
        {
            return false;
        }
    }

    /// <summary>The JSON header of a <c>.tdelta</c>: <c>TDLT</c>, u32 version, u32 length, JSON.</summary>
    private static JsonDocument ReadHeader(Stream delta)
    {
        var fixedPart = new byte[12];
        delta.ReadExactly(fixedPart);
        if (fixedPart[0] != (byte)'T' || fixedPart[1] != (byte)'D' || fixedPart[2] != (byte)'L' || fixedPart[3] != (byte)'T')
            throw new Exception("not a Tempest delta (bad magic)");
        var version = BitConverter.ToUInt32(fixedPart, 4);
        if (version != 1) throw new Exception($"delta version {version} is not supported by this Tempest");
        var length = BitConverter.ToInt32(fixedPart, 8);
        if (length < 2 || length > 256 * 1024 * 1024) throw new Exception("delta header length out of range");
        var json = new byte[length];
        delta.ReadExactly(json);
        return JsonDocument.Parse(json);
    }

    // ----- the upk sidecar ----------------------------------------------------------------

    /// <summary>
    /// The <c>upk</c> binary: <c>TEMPEST_UPK</c>, next to this executable (the launcher bundles it with
    /// tempest-cli), or the development build in the repository.
    /// </summary>
    public static string FindUpk()
    {
        var name = OperatingSystem.IsWindows() ? "upk.exe" : "upk";
        if (Environment.GetEnvironmentVariable("TEMPEST_UPK") is { Length: > 0 } env && File.Exists(env)) return env;
        var beside = Path.Combine(AppContext.BaseDirectory, name);
        if (File.Exists(beside)) return beside;
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir != null; dir = dir.Parent)
        {
            var dev = Path.Combine(dir.FullName, "Tempest.UnrealScript", "target", "release", name);
            if (File.Exists(dev)) return dev;
        }
        throw new Exception("the upk tool that applies mod deltas is not installed next to tempest-cli (set TEMPEST_UPK to its path)");
    }

    private static async Task<(int Code, string Stderr)> RunUpkAsync(IEnumerable<string> args)
    {
        var psi = new ProcessStartInfo(FindUpk()) { RedirectStandardError = true, RedirectStandardOutput = true, UseShellExecute = false, CreateNoWindow = true };
        foreach (var a in args) psi.ArgumentList.Add(a);
        using var process = Process.Start(psi) ?? throw new Exception("could not start upk");
        var stderr = process.StandardError.ReadToEndAsync();
        var stdout = process.StandardOutput.ReadToEndAsync();
        await process.WaitForExitAsync();
        return (process.ExitCode, (await stderr) + (await stdout));
    }

    // ----- state ----------------------------------------------------------------------------

    public sealed class ResolvedFile
    {
        /// <summary>Hash of what the file was built from: base identity and every delta, in order.</summary>
        public string Stack { get; set; } = "";
        /// <summary>SHA-256 of the file as written.</summary>
        public string Output { get; set; } = "";
    }

    private static string StatePath(string resolvedGame) => Path.Combine(resolvedGame, ".tempest", "v2", "deltas.json");

    private static Dictionary<string, ResolvedFile> LoadState(string resolvedGame)
    {
        try
        {
            var path = StatePath(resolvedGame);
            if (!File.Exists(path)) return new Dictionary<string, ResolvedFile>(StringComparer.OrdinalIgnoreCase);
            var state = JsonSerializer.Deserialize(File.ReadAllText(path), DeltaJsonContext.Default.DictionaryStringResolvedFile);
            return new Dictionary<string, ResolvedFile>(state ?? [], StringComparer.OrdinalIgnoreCase);
        }
        catch
        {
            return new Dictionary<string, ResolvedFile>(StringComparer.OrdinalIgnoreCase);
        }
    }

    private static void SaveState(string resolvedGame, Dictionary<string, ResolvedFile> state)
    {
        var path = StatePath(resolvedGame);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, JsonSerializer.Serialize(state, DeltaJsonContext.Default.DictionaryStringResolvedFile));
    }

    public sealed class Report
    {
        public List<string> Rebuilt { get; } = [];
        public List<string> Restored { get; } = [];
        /// <summary>Objects or rows a delta replaced that another mod (or a game update) had changed first; the later mod won.</summary>
        public List<string> Drift { get; } = [];
        public List<string> Warnings { get; } = [];
    }

    private sealed record Contribution(ModRecord Mod, string DeltaFile);

    // ----- resolving --------------------------------------------------------------------------

    /// <summary>
    /// Bring every delta-managed game file up to date with the enabled mods. Call under the
    /// mod lock, after mods.json was saved. Never throws: problems are reported.
    /// </summary>
    public static async Task<Report> ResolveAsync(string gamePath)
    {
        var report = new Report();
        try
        {
            var resolvedGame = GameFolderResolver.Resolve(gamePath);
            var state = LoadState(resolvedGame);
            var mods = File.Exists(ModCommands.GetMetadataPath(gamePath)) ? ModCommands.LoadMetadata(gamePath) : [];
            var v2 = mods.Where(m => string.Equals(m.Kind, "V2", StringComparison.OrdinalIgnoreCase)).ToList();

            // Who contributes what, in install order.
            var contributions = new Dictionary<string, List<Contribution>>(StringComparer.OrdinalIgnoreCase);
            foreach (var mod in v2.Where(m => m.Enabled).OrderBy(m => m.InstalledAt ?? DateTime.MinValue).ThenBy(m => m.Id, StringComparer.OrdinalIgnoreCase))
            {
                var modDir = TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, mod.Id);
                foreach (var file in mod.DeltaFiles)
                {
                    var relative = Relative(resolvedGame, file);
                    var deltaFile = Path.Combine(modDir, "deltas", relative + Suffix);
                    if (!File.Exists(deltaFile))
                    {
                        report.Warnings.Add($"'{mod.Name}': its delta for {relative} is missing from the mod folder");
                        continue;
                    }
                    if (!contributions.TryGetValue(relative, out var list)) contributions[relative] = list = [];
                    list.Add(new Contribution(mod, deltaFile));
                }
            }
            // Whole-file owners: their copy is the base the deltas go on.
            var owners = new Dictionary<string, ModRecord>(StringComparer.OrdinalIgnoreCase);
            foreach (var mod in v2.Where(m => m.Enabled))
            {
                foreach (var file in mod.OwnedFiles) owners[Relative(resolvedGame, file)] = mod;
            }

            var files = new HashSet<string>(contributions.Keys, StringComparer.OrdinalIgnoreCase);
            files.UnionWith(state.Keys);
            if (files.Count == 0) return report;

            // Script packages gaining classes first: the class database the other
            // packages decode against is read from disk.
            var ordered = files
                .OrderByDescending(f => contributions.TryGetValue(f, out var c) && c.Any(x => AddsClasses(x.DeltaFile)))
                .ThenBy(f => f, StringComparer.OrdinalIgnoreCase)
                .ToList();

            var changed = false;
            foreach (var relative in ordered)
            {
                var gameFile = Path.GetFullPath(Path.Combine(resolvedGame, relative));
                var backup = TempestPathUtility.GetLocalV2BackupPath(resolvedGame, relative);
                contributions.TryGetValue(relative, out var stack);
                owners.TryGetValue(relative, out var owner);

                if (stack == null || stack.Count == 0)
                {
                    // No delta touches it any more: back to the base.
                    if (state.Remove(relative))
                    {
                        changed = true;
                        if (owner != null)
                        {
                            // The owner's own copy is the whole file again.
                            var snapshot = Path.Combine(TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, owner.Id), "files", relative);
                            if (File.Exists(snapshot)) File.Copy(snapshot, gameFile, overwrite: true);
                        }
                        else if (File.Exists(backup))
                        {
                            File.Copy(backup, gameFile, overwrite: true);
                            File.Delete(backup);
                        }
                        report.Restored.Add(relative);
                    }
                    continue;
                }

                // The base: the owner's copy, else the pristine file (kept as the backup
                // from the moment a delta first touches it, like a whole-file install does).
                string basePath;
                string baseId;
                if (owner != null)
                {
                    basePath = Path.Combine(TempestPathUtility.GetLocalV2ModDirectory(resolvedGame, owner.Id), "files", relative);
                    baseId = $"mod:{owner.Id}@{owner.Version}:{HashFile(basePath)}";
                    if (!File.Exists(backup) && File.Exists(gameFile) && !state.ContainsKey(relative))
                    {
                        // Never resolved and owned: the owner's install already backed the original up.
                    }
                }
                else
                {
                    if (!File.Exists(backup))
                    {
                        if (!File.Exists(gameFile))
                        {
                            report.Warnings.Add($"{relative}: the game has no such file, the deltas of {string.Join(", ", stack.Select(c => $"'{c.Mod.Name}'"))} cannot be applied");
                            continue;
                        }
                        if (state.ContainsKey(relative))
                        {
                            // Built before, but the backup went missing: the file on disk is modded; nothing better to start from.
                            report.Warnings.Add($"{relative}: the original backup is missing; the current file is taken as the original");
                        }
                        Directory.CreateDirectory(Path.GetDirectoryName(backup)!);
                        File.Copy(gameFile, backup);
                    }
                    basePath = backup;
                    baseId = $"pristine:{HashFile(backup)}";
                }

                var stackHash = StackHash(baseId, stack);
                if (state.TryGetValue(relative, out var previous) && previous.Stack == stackHash && File.Exists(gameFile) && HashFile(gameFile) == previous.Output)
                    continue;

                var tmp = gameFile + ".tempest-resolving";
                try
                {
                    var args = new List<string> { "delta", "apply", basePath };
                    args.AddRange(stack.Select(c => c.DeltaFile));
                    args.Add("-o");
                    args.Add(tmp);
                    if (!relative.EndsWith(".dat", StringComparison.OrdinalIgnoreCase))
                    {
                        // Reflection comes from the game's script packages, where the file lives (the backup folder has none).
                        args.Add("-I");
                        args.Add(Path.GetDirectoryName(gameFile)!);
                    }
                    var (code, output) = await RunUpkAsync(args);
                    foreach (var line in output.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
                    {
                        if (line.StartsWith("drift:", StringComparison.Ordinal)) report.Drift.Add($"{relative}: {line["drift:".Length..].Trim()}");
                        else if (line.StartsWith("warning:", StringComparison.Ordinal)) report.Warnings.Add($"{relative}: {line["warning:".Length..].Trim()}");
                    }
                    if (code != 0 || !File.Exists(tmp))
                        throw new Exception(output.Split('\n').LastOrDefault(l => l.Contains("error", StringComparison.OrdinalIgnoreCase))?.Trim() ?? $"upk exited {code}");
                    Directory.CreateDirectory(Path.GetDirectoryName(gameFile)!);
                    File.Move(tmp, gameFile, overwrite: true);
                    state[relative] = new ResolvedFile { Stack = stackHash, Output = HashFile(gameFile) };
                    changed = true;
                    report.Rebuilt.Add(relative);
                }
                catch (Exception ex)
                {
                    try { if (File.Exists(tmp)) File.Delete(tmp); } catch { }
                    report.Warnings.Add($"{relative}: could not apply the deltas of {string.Join(", ", stack.Select(c => $"'{c.Mod.Name}'"))}: {ex.Message}");
                }
            }
            if (changed) SaveState(resolvedGame, state);
        }
        catch (Exception ex)
        {
            report.Warnings.Add($"could not resolve mod deltas: {ex.Message}");
        }
        return report;
    }

    /// <summary>Runs the resolver and prints what it did on stderr (JSON outputs on stdout stay clean).</summary>
    public static async Task ResolveAndReportAsync(string gamePath)
    {
        var report = await ResolveAsync(gamePath);
        foreach (var f in report.Rebuilt) await Console.Error.WriteLineAsync($"[deltas] rebuilt {f}");
        foreach (var f in report.Restored) await Console.Error.WriteLineAsync($"[deltas] restored {f}");
        foreach (var d in report.Drift) await Console.Error.WriteLineAsync($"[deltas] note: {d}");
        foreach (var w in report.Warnings) await Console.Error.WriteLineAsync($"Warning: {w}");
    }

    private static string StackHash(string baseId, List<Contribution> stack)
    {
        var sb = new StringBuilder(baseId);
        foreach (var c in stack) sb.Append('\n').Append(c.Mod.Id).Append('@').Append(c.Mod.Version).Append(':').Append(HashFile(c.DeltaFile));
        return Convert.ToHexStringLower(SHA256.HashData(Encoding.UTF8.GetBytes(sb.ToString())));
    }

    private static string HashFile(string path)
    {
        using var stream = File.OpenRead(path);
        return Convert.ToHexStringLower(SHA256.HashData(stream));
    }

    private static string Relative(string resolvedGame, string file)
    {
        var relative = Path.IsPathRooted(file) ? Path.GetRelativePath(resolvedGame, file) : file;
        return relative.Replace('\\', '/');
    }
}

[JsonSerializable(typeof(Dictionary<string, DeltaResolver.ResolvedFile>))]
internal partial class DeltaJsonContext : JsonSerializerContext
{
}
