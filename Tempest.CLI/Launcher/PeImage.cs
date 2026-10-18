namespace Tempest.CLI.Launcher;

/// <summary>Reads just enough of a PE header to tell a Win32 image from a Win64 one.</summary>
internal static class PeImage
{
    private const ushort MachineI386 = 0x014c;
    private const ushort MachineAmd64 = 0x8664;

    /// <summary>
    /// <c>true</c> for an x64 image, <c>false</c> for x86, <c>null</c> when the file cannot be read
    /// or is not a PE image (the injector then reports whatever LoadLibrary says).
    /// </summary>
    public static bool? Is64Bit(string path)
    {
        try
        {
            using var fs = File.OpenRead(path);
            using var reader = new BinaryReader(fs);
            if (fs.Length < 0x40 || reader.ReadUInt16() != 0x5A4D) // "MZ"
                return null;
            fs.Position = 0x3C;
            var peOffset = reader.ReadUInt32();
            if (peOffset + 6 > fs.Length)
                return null;
            fs.Position = peOffset;
            if (reader.ReadUInt32() != 0x00004550) // "PE\0\0"
                return null;
            return reader.ReadUInt16() switch
            {
                MachineAmd64 => true,
                MachineI386 => false,
                _ => null,
            };
        }
        catch (IOException)
        {
            return null;
        }
        catch (UnauthorizedAccessException)
        {
            return null;
        }
    }
}
