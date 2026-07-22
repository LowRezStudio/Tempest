namespace Tempest.CLI.Mods;

public interface IModInstaller
{
    Task<ModInstallResult> InstallAsync(string gamePath, string modFilePath, bool replace, bool allowUnsigned);
    Task RemoveAsync(string gamePath, ModRecord mod);
    Task EnableAsync(string gamePath, ModRecord mod);
    Task DisableAsync(string gamePath, ModRecord mod);
}
