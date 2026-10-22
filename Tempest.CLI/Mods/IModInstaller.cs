namespace Tempest.CLI.Mods;

public interface IModInstaller
{
    /// <summary>Enters <paramref name="modLock"/> before reading or changing installed mods and leaves it held, so the caller records the result under it.</summary>
    Task<ModInstallResult> InstallAsync(string gamePath, string modFilePath, bool replace, bool stack, bool allowUntrusted, ModLock modLock);
    Task RemoveAsync(string gamePath, ModRecord mod);
    Task EnableAsync(string gamePath, ModRecord mod);
    Task DisableAsync(string gamePath, ModRecord mod);
}
