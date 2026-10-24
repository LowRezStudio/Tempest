using System.ComponentModel;

namespace Tempest.CLI.Launcher;

/// <summary>Recognises files an antivirus refused to open or run, or quarantined.</summary>
internal static class Antivirus
{
    private const int ERROR_VIRUS_INFECTED = 225;
    private const int ERROR_VIRUS_DELETED = 226;

    public static bool IsBlock(Exception ex) => ex switch
    {
        Win32Exception w => w.NativeErrorCode is ERROR_VIRUS_INFECTED or ERROR_VIRUS_DELETED,
        IOException io => io.HResult == HResultFromWin32(ERROR_VIRUS_INFECTED) || io.HResult == HResultFromWin32(ERROR_VIRUS_DELETED),
        _ => false,
    };

    /// <summary>Whether a file that existed a moment ago is gone, which on Windows means it was quarantined.</summary>
    public static bool Removed(string path) => OperatingSystem.IsWindows() && !File.Exists(path);

    public static bool Refuses(string path)
    {
        if (!OperatingSystem.IsWindows() || !File.Exists(path))
            return false;
        try
        {
            using var fs = File.OpenRead(path);
            fs.ReadByte();
            return false;
        }
        catch (IOException ex)
        {
            return IsBlock(ex);
        }
        catch (UnauthorizedAccessException)
        {
            return false;
        }
    }

    /// <summary>The launcher watches stderr for this line and offers to add the folder to Defender's exclusions.</summary>
    public static void Report(string folder) => Console.Error.WriteLine($"[av-blocked] {Path.GetFullPath(folder)}");

    private static int HResultFromWin32(int code) => unchecked((int)0x80070000) | code;
}
