namespace Tempest.CLI;

internal class WineCommands
{
    public async Task Init()
    {
        await WineUtility.WaitForPrefix();
    }
}