using System;

namespace Tempest.CLI.Mods;

public static class ModInstallerFactory
{
    public static IModInstaller Create(ModFormatVersion version)
    {
        return version switch
        {
            ModFormatVersion.V1 => new ModV1Installer(),
            ModFormatVersion.V2 => new ModV2Installer(),
            _ => throw new NotSupportedException($"Mod format version '{version}' is not supported.")
        };
    }

    public static IModInstaller CreateForFile(string filePath)
    {
        var version = ModFormatDetector.Detect(filePath);
        return Create(version);
    }
}
