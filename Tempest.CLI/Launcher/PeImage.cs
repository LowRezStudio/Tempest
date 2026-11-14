namespace Tempest.CLI.Launcher;

/// <summary>Reads just enough of a PE header to tell a Win32 image from a Win64 one, and one build from another.</summary>
internal static class PeImage
{
    private const ushort MachineI386 = 0x014c;
    private const ushort MachineAmd64 = 0x8664;

    /// <summary>
    /// <c>true</c> for an x64 image, <c>false</c> for x86, <c>null</c> when the file cannot be read
    /// or is not a PE image (the injector then reports whatever LoadLibrary says).
    /// </summary>
    public static bool? Is64Bit(string path) => ReadFileHeader(path)?.Machine switch
    {
        MachineAmd64 => true,
        MachineI386 => false,
        _ => null,
    };

    /// <summary>The link timestamp, which tells one build of an executable from another; <c>null</c> as above.</summary>
    public static uint? Timestamp(string path) => ReadFileHeader(path)?.Timestamp;

    private static (ushort Machine, uint Timestamp)? ReadFileHeader(string path)
    {
        try
        {
            using var fs = File.OpenRead(path);
            using var reader = new BinaryReader(fs);
            if (fs.Length < 0x40 || reader.ReadUInt16() != 0x5A4D) // "MZ"
                return null;
            fs.Position = 0x3C;
            var peOffset = reader.ReadUInt32();
            if (peOffset + 12 > fs.Length)
                return null;
            fs.Position = peOffset;
            if (reader.ReadUInt32() != 0x00004550) // "PE\0\0"
                return null;
            var machine = reader.ReadUInt16();
            reader.ReadUInt16(); // NumberOfSections
            return (machine, reader.ReadUInt32());
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
