namespace Tempest.CLI.Mods;

/// <summary>Semver precedence (1.2.3, 1.2.3-beta.1); mirrors compareVersions in the repository.</summary>
public sealed class SemverComparer : IComparer<string>
{
    public static readonly SemverComparer Instance = new();

    public int Compare(string? a, string? b)
    {
        if (a == null || b == null) return string.CompareOrdinal(a, b);
        var (ca, pa) = Split(a);
        var (cb, pb) = Split(b);
        for (var i = 0; i < 3; i++)
        {
            var d = ca[i].CompareTo(cb[i]);
            if (d != 0) return d;
        }
        if (pa == pb) return 0;
        if (pa == null) return 1;
        if (pb == null) return -1;
        var xs = pa.Split('.');
        var ys = pb.Split('.');
        for (var i = 0; i < Math.Max(xs.Length, ys.Length); i++)
        {
            if (i >= xs.Length) return -1;
            if (i >= ys.Length) return 1;
            var nx = long.TryParse(xs[i], out var vx);
            var ny = long.TryParse(ys[i], out var vy);
            if (nx && ny) { var d = vx.CompareTo(vy); if (d != 0) return d; }
            else if (nx != ny) return nx ? -1 : 1;
            else { var d = string.CompareOrdinal(xs[i], ys[i]); if (d != 0) return d; }
        }
        return 0;
    }

    private static (long[] core, string? pre) Split(string v)
    {
        var dash = v.IndexOf('-');
        var core = (dash < 0 ? v : v[..dash]).Split('.');
        var nums = new long[3];
        for (var i = 0; i < 3 && i < core.Length; i++) long.TryParse(core[i], out nums[i]);
        return (nums, dash < 0 ? null : v[(dash + 1)..]);
    }
}
