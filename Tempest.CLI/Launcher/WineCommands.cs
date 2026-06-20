using Tempest.CLI.Extensions;

namespace Tempest.CLI.Launcher;

internal class WineCommands
{
    public static async Task Init()
    {
        await WineExtensions.WaitForPrefix();
    }
}