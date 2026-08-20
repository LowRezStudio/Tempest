using Tempest.CLI.Extensions;

namespace Tempest.CLI.Launcher;

internal class WineCommands
{
    public async Task Init()
    {
        await WineExtensions.WaitForPrefix();
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
