using System.Threading.Tasks;

namespace Tempest.CLI.Mods;

public interface IModInstaller
{
    Task<ModInstallResult> InstallAsync(string gamePath, string modFilePath, bool replace);
    Task RemoveAsync(string gamePath, ModRecord mod);
}
