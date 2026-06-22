namespace Tempest.CLI.Mods;

public static class ModFormatDetector
{
    public static ModFormatVersion Detect(string filePath)
    {
        var ext = Path.GetExtension(filePath).ToLowerInvariant();
        return ext switch
        {
            ".upk" or ".pck" => ModFormatVersion.V1,
            ".zip" or ".tempest" => ModFormatVersion.V2,
            _ => throw new NotSupportedException(
                $"Unsupported mod format: '{ext}'. Only V1 (.upk, .pck) and V2 (.zip, .tempest) are supported.")
        };
    }
}
