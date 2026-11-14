namespace Tempest.CLI.Launcher;

/// <summary>
/// The startup fixes of Tempest.Utils (<c>faststart</c>): injected into the clients they were
/// written against, on by default.
/// </summary>
internal static class FastStartup
{
    public const string DllName = "faststart-windows_x86_64.dll";

    // Link timestamp of the 8.1 Win64 client. The DLL checks it again, and every site it patches.
    private const uint Paladins81Win64 = 0x679BFE80;

    public static bool Supports(string exePath) =>
        PeImage.Is64Bit(exePath) == true && PeImage.Timestamp(exePath) == Paladins81Win64;
}
