using System;
using System.IO;

namespace Tempest.CLI.Mods;

public static class ModFormatDetector
{
    public static ModFormatVersion Detect(string filePath)
    {
        var ext = Path.GetExtension(filePath).ToLowerInvariant();
        if (ext == ".upk" || ext == ".pck")
        {
            return ModFormatVersion.V1;
        }

        throw new NotSupportedException($"Unsupported mod format: '{ext}'. Only V1 (.upk, .pck) is supported currently.");
    }
}
