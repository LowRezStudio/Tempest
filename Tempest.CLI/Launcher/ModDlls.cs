namespace Tempest.CLI.Launcher;

/// <summary>
/// Which DLLs of a mod's <c>dlls/</c> folder belong in a given game process.
/// </summary>
/// <remarks>
/// Layout: <c>dlls/64/</c> holds Win64 DLLs, <c>dlls/32/</c> Win32 ones, and anything
/// directly in <c>dlls/</c> (or in any other subfolder) is the pre-split layout, injected
/// into either client. The PE machine type is checked as a safety net in every case, so a
/// misplaced DLL is skipped with a message instead of failing to load in the game.
/// </remarks>
internal static class ModDlls
{
    public const string Win64Folder = "64";
    public const string Win32Folder = "32";

    /// <summary>DLLs under <paramref name="dllsDir"/> that should be injected into a Win64 (or Win32) process.</summary>
    public static List<string> ForProcess(string dllsDir, bool is64Bit)
    {
        var result = new List<string>();
        if (!Directory.Exists(dllsDir))
            return result;

        var skipFolder = Path.Combine(dllsDir, is64Bit ? Win32Folder : Win64Folder);
        foreach (var dll in Directory.GetFiles(dllsDir, "*.dll", SearchOption.AllDirectories))
        {
            var dir = Path.GetDirectoryName(dll) ?? dllsDir;
            if (IsSameOrBelow(dir, skipFolder))
                continue; // the other client's folder

            var dll64 = PeImage.Is64Bit(dll);
            if (dll64 is not null && dll64 != is64Bit)
            {
                Console.Error.WriteLine($"[inject] skipping {Path.GetRelativePath(dllsDir, dll)}: built for {(dll64.Value ? "Win64" : "Win32")}, the game is {(is64Bit ? "Win64" : "Win32")}");
                continue;
            }
            result.Add(dll);
        }
        return result;
    }

    private static bool IsSameOrBelow(string dir, string ancestor)
    {
        var rel = Path.GetRelativePath(ancestor, dir);
        return rel == "." || (!rel.StartsWith("..", StringComparison.Ordinal) && !Path.IsPathRooted(rel));
    }
}
