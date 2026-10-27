using System.Diagnostics;
using System.Globalization;
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

    /// <summary>
    /// Rebuilds every file of <paramref name="jobs"/> in one <c>upk delta batch</c> run, so the
    /// class database is loaded once, and moves the outputs over the game files once it exited.
    /// </summary>
    private static async Task RebuildAsync(List<Job> jobs, Dictionary<string, ResolvedFile> state, Report report, Action<double, string>? progress)
    {
        var plan = Path.Combine(Path.GetTempPath(), $"tempest-deltas-{Environment.ProcessId}.json");
        using (var stream = File.Create(plan))
        using (var json = new Utf8JsonWriter(stream))
        {
            json.WriteStartObject();
            json.WriteStartArray("jobs");
            foreach (var job in jobs)
            {
                json.WriteStartObject();
                json.WriteString("base", job.BasePath);
                json.WriteStartArray("deltas");
                foreach (var c in job.Stack) json.WriteStringValue(c.DeltaFile);
                json.WriteEndArray();
                json.WriteString("out", job.Tmp);
                // Script packages, or a marshal's token tables, come from the game file's folder (the backup folder has none).
                json.WriteString("dir", Path.GetDirectoryName(job.GameFile));
                json.WriteString("game", job.GameFile);
                json.WriteEndObject();
            }
            json.WriteEndArray();
            json.WriteEndObject();
        }

        var results = new string?[jobs.Count];
        string stderr;
        int code;
        try
        {
            var psi = new ProcessStartInfo(FindUpk()) { RedirectStandardError = true, RedirectStandardOutput = true, UseShellExecute = false, CreateNoWindow = true };
            foreach (var a in new[] { "delta", "batch", plan }) psi.ArgumentList.Add(a);
            using var process = Process.Start(psi) ?? throw new Exception("could not start upk");
            var stderrTask = process.StandardError.ReadToEndAsync();
            while (await process.StandardOutput.ReadLineAsync() is { } line)
            {
                JsonDocument doc;
                try { doc = JsonDocument.Parse(line); }
                catch (JsonException) { continue; }
                using (doc)
                {
                    var e = doc.RootElement;
                    var i = e.TryGetProperty("job", out var j) && j.TryGetInt32(out var n) ? n : -1;
                    var job = i >= 0 && i < jobs.Count ? jobs[i] : null;
                    var text = (string name) => e.TryGetProperty(name, out var v) ? v.GetString() ?? "" : "";
                    switch (text("event"))
                    {
                        case "progress":
                            progress?.Invoke(e.GetProperty("fraction").GetDouble(), Describe(text("stage"), job, i, jobs.Count));
                            break;
                        case "warning" when job != null:
                            report.Warnings.Add($"{job.Relative}: {text("text")}");
                            break;
                        case "drift" when job != null:
                            report.Drift.Add($"{job.Relative}: {text("text")} was changed since the delta was made; the delta's version won");
                            break;
                        case "done" when job != null:
                            results[i] = "";
                            break;
                        case "error" when job != null:
                            results[i] = text("message");
                            break;
                    }
                }
            }
            await process.WaitForExitAsync();
            stderr = await stderrTask;
            code = process.ExitCode;
        }
        catch (Exception ex)
        {
            stderr = $"error: {ex.Message}";
            code = -1;
        }
        finally
        {
            try { File.Delete(plan); } catch { }
        }

        for (var i = 0; i < jobs.Count; i++)
        {
            var job = jobs[i];
            try
            {
                if (results[i] is { Length: > 0 } message) throw new Exception(message);
                if (results[i] == null || !File.Exists(job.Tmp))
                    throw new Exception(stderr.Split('\n').LastOrDefault(l => l.Contains("error", StringComparison.OrdinalIgnoreCase))?.Trim() ?? $"upk exited {code}");
                Directory.CreateDirectory(Path.GetDirectoryName(job.GameFile)!);
                File.Move(job.Tmp, job.GameFile, overwrite: true);
                state[job.Relative] = new ResolvedFile { Stack = job.StackHash, Output = HashFile(job.GameFile) };
                report.Rebuilt.Add(job.Relative);
            }
            catch (Exception ex)
            {
                try { if (File.Exists(job.Tmp)) File.Delete(job.Tmp); } catch { }
                report.Warnings.Add($"{job.Relative}: could not apply the deltas of {string.Join(", ", job.Stack.Select(c => $"'{c.Mod.Name}'"))}: {ex.Message}");
            }
        }
    }

    private static string Describe(string stage, Job? job, int index, int count)
    {
        var file = job == null ? "" : $"{Path.GetFileName(job.Relative)} ({index + 1}/{count})";
        return stage switch
        {
            "classes" => "Loading the game's classes",
            "open" => $"Reading {file}",
            "apply" => $"Applying changes to {file}",
            "save" => $"Writing {file}",
            _ => "Done",
        };
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

    /// <summary>A file whose stack changed since it was last built.</summary>
    private sealed record Job(string Relative, string GameFile, string BasePath, List<Contribution> Stack, string StackHash)
    {
        public string Tmp => GameFile + ".tempest-resolving";
    }

    // ----- resolving --------------------------------------------------------------------------

    /// <summary>
    /// Bring every delta-managed game file up to date with the enabled mods. Call under the
    /// mod lock, after mods.json was saved. Never throws: problems are reported.
    /// <paramref name="progress"/> hears the fraction done and what is being worked on while
    /// files are rebuilt (nothing when every file is up to date).
    /// </summary>
    public static async Task<Report> ResolveAsync(string gamePath, Action<double, string>? progress = null)
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
            var jobs = new List<Job>();
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
                jobs.Add(new Job(relative, gameFile, basePath, stack, stackHash));
            }
            if (jobs.Count > 0)
            {
                await RebuildAsync(jobs, state, report, progress);
                changed = true;
            }
            if (changed) SaveState(resolvedGame, state);
        }
        catch (Exception ex)
        {
            report.Warnings.Add($"could not resolve mod deltas: {ex.Message}");
        }
        return report;
    }

    /// <summary>
    /// Runs the resolver and prints what it did on stderr (JSON outputs on stdout stay clean),
    /// with <c>[progress] 42% what</c> lines while it rebuilds files (the launcher's progress bar).
    /// </summary>
    public static async Task ResolveAndReportAsync(string gamePath)
    {
        var report = await ResolveAsync(gamePath, (fraction, what) => Console.Error.WriteLine(string.Create(CultureInfo.InvariantCulture, $"[progress] {Math.Floor(fraction * 100)}% {what}")));
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
