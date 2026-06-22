namespace Tempest.CLI.Mods;

public interface IModInstaller
{
    Task<ModInstallResult> InstallAsync(string gamePath, string modFilePath, bool replace, bool allowUnsigned);
    Task RemoveAsync(string gamePath, ModRecord mod);
}
