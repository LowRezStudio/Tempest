using Tempest.CLI.Extensions;

namespace Tempest.CLI.Launcher;

internal class WineCommands
{
    public async Task Init()
    {
        await WineExtensions.WaitForPrefix();
    }

    /// <summary>Prints the auto-detected Proton installation directory (or 'none') to stdout.</summary>
    public void DetectProton()
    {
        var detected = WineExtensions.DetectProtonDirectory();
        Console.WriteLine(detected ?? "none");
    }

    /// <summary>Prints every detected Proton installation, one directory per line.</summary>
    public void ListProton()
    {
        foreach (var dir in WineExtensions.EnumerateProtonDirectories())
        {
            Console.WriteLine(dir);
        }
    }
}
