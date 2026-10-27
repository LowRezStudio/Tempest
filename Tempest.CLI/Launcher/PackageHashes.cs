using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using Tempest.CLI.Mods;

namespace Tempest.CLI.Launcher;

/// <summary>
/// The client carries a SHA-1 of every script package (and of the startup maps and some shaders) as
/// RCDATA resource 1010, <c>name\0</c> followed by 20 hash bytes per file, and refuses to start when a
/// listed file differs ("SHA Verification failed ... Bad hash"). FSHA1::InitializeFileHashesFromBuffer
/// keeps pointers into the resource rather than copies, so a mod's replacement of a listed file is let
/// through by writing its hash over the entry in the started process before the file is opened.
/// </summary>
internal static class PackageHashes
{
    private const uint ResourceTypeRcData = 10;
    private const uint HashTableResourceId = 1010;
    private const int HashLength = 20;

    public readonly record struct Patch(string Name, uint Rva, byte[] Hash);

    /// <summary>Hashes to write for the listed files that enabled mods replaced.</summary>
    public static List<Patch> ForMods(string exePath, string gamePath)
    {
        if (!File.Exists(ModCommands.GetMetadataPath(gamePath))) return [];
        var modFiles = ModCommands.LoadMetadata(gamePath)
            .Where(m => m.Enabled)
            .SelectMany(m => m.OwnedFiles.Concat(m.DeltaFiles))
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToList();
        if (modFiles.Count == 0) return [];

        var table = ReadTable(exePath);
        var patches = new List<Patch>();
        foreach (var file in modFiles)
        {
            var name = Path.GetFileName(file);
            if (!table.TryGetValue(name, out var entry) || !File.Exists(file)) continue;
            using var stream = File.OpenRead(file);
            var hash = SHA1.HashData(stream);
            if (!hash.AsSpan().SequenceEqual(entry.Hash))
                patches.Add(new Patch(name, entry.Rva, hash));
        }
        return patches;
    }

    /// <summary>Writes the hashes into the started game, which only a native Windows launch allows.</summary>
    public static void Apply(Process process, IReadOnlyList<Patch> patches)
    {
        if (patches.Count == 0) return;
        var names = string.Join(", ", patches.Select(p => p.Name));
        if (!OperatingSystem.IsWindows() || !Environment.Is64BitProcess)
        {
            Console.Error.WriteLine($"[hashes] {names} replaced by a mod: the game will refuse to load it (its SHA-1 check can only be updated on Windows)");
            return;
        }

        var info = new ProcessBasicInformation();
        // PEB.ImageBaseAddress (+0x10 in the native PEB, also for a WOW64 client).
        if (NtQueryInformationProcess(process.Handle, 0, ref info, System.Runtime.InteropServices.Marshal.SizeOf<ProcessBasicInformation>(), out _) != 0 ||
            !ReadProcessMemory(process.Handle, info.PebBaseAddress + 0x10, out var imageBase, IntPtr.Size, out _))
        {
            Console.Error.WriteLine($"[hashes] could not find the game's image to update the SHA-1 of {names} (error {System.Runtime.InteropServices.Marshal.GetLastWin32Error()})");
            return;
        }

        foreach (var patch in patches)
        {
            var address = imageBase + (nint)patch.Rva;
            var ok = VirtualProtectEx(process.Handle, address, HashLength, PageReadWrite, out var previous) &&
                     WriteProcessMemory(process.Handle, address, patch.Hash, HashLength, out _);
            var error = System.Runtime.InteropServices.Marshal.GetLastWin32Error();
            if (ok) VirtualProtectEx(process.Handle, address, HashLength, previous, out _);
            Console.Error.WriteLine(ok
                ? $"[hashes] {patch.Name}: the game now expects the mod's copy ({Convert.ToHexStringLower(patch.Hash)})"
                : $"[hashes] {patch.Name}: could not update the game's SHA-1 (error {error})");
        }
    }

    /// <summary>File name (as listed, lower case) to the RVA and value of its hash.</summary>
    private static Dictionary<string, (uint Rva, byte[] Hash)> ReadTable(string exePath)
    {
        var table = new Dictionary<string, (uint, byte[])>(StringComparer.OrdinalIgnoreCase);
        using var fs = File.OpenRead(exePath);
        using var reader = new BinaryReader(fs);
        uint U32(long at)
        {
            fs.Position = at;
            return reader.ReadUInt32();
        }
        ushort U16(long at)
        {
            fs.Position = at;
            return reader.ReadUInt16();
        }

        var pe = U32(0x3C);
        if (U32(pe) != 0x00004550) return table; // "PE\0\0"
        var sectionCount = U16(pe + 6);
        var optionalHeader = pe + 24;
        var dataDirectories = optionalHeader + (U16(optionalHeader) == 0x20B ? 112 : 96);
        var resourceRva = U32(dataDirectories + 2 * 8);
        if (resourceRva == 0) return table;
        var sectionTable = optionalHeader + U16(pe + 20);

        long Offset(uint rva)
        {
            for (var i = 0; i < sectionCount; i++)
            {
                var section = sectionTable + i * 40;
                var start = U32(section + 12);
                if (rva >= start && rva < start + Math.Max(U32(section + 8), U32(section + 16)))
                    return U32(section + 20) + (rva - start);
            }
            throw new InvalidDataException($"RVA 0x{rva:x} lies outside every section of {exePath}");
        }

        var root = Offset(resourceRva);
        // One level of the resource tree: the entry with this numeric id, or the first entry.
        long Child(long directory, uint? id)
        {
            var count = U16(directory + 12) + U16(directory + 14);
            for (var i = 0; i < count; i++)
            {
                var entry = directory + 16 + i * 8;
                if (id is { } wanted && U32(entry) != wanted) continue;
                return root + (U32(entry + 4) & 0x7FFF_FFFF);
            }
            return -1;
        }

        var type = Child(root, ResourceTypeRcData);
        var name = type < 0 ? -1 : Child(type, HashTableResourceId);
        var dataEntry = name < 0 ? -1 : Child(name, null);
        if (dataEntry < 0) return table;
        var dataRva = U32(dataEntry);
        var data = new byte[U32(dataEntry + 4)];
        fs.Position = Offset(dataRva);
        fs.ReadExactly(data);

        for (var p = 0; p < data.Length;)
        {
            var end = Array.IndexOf(data, (byte)0, p);
            if (end <= p || end + 1 + HashLength > data.Length) break;
            table[Encoding.ASCII.GetString(data, p, end - p)] = ((uint)(dataRva + end + 1), data[(end + 1)..(end + 1 + HashLength)]);
            p = end + 1 + HashLength;
        }
        return table;
    }

    private const uint PageReadWrite = 0x04;

    [StructLayout(LayoutKind.Sequential)]
    private struct ProcessBasicInformation
    {
        public IntPtr ExitStatus;
        public IntPtr PebBaseAddress;
        public IntPtr AffinityMask;
        public IntPtr BasePriority;
        public IntPtr UniqueProcessId;
        public IntPtr InheritedFromUniqueProcessId;
    }

    [DllImport("ntdll.dll")]
    private static extern int NtQueryInformationProcess(IntPtr processHandle, int processInformationClass, ref ProcessBasicInformation processInformation, int processInformationLength, out int returnLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool ReadProcessMemory(IntPtr hProcess, IntPtr lpBaseAddress, out IntPtr lpBuffer, nint nSize, out nint lpNumberOfBytesRead);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool WriteProcessMemory(IntPtr hProcess, IntPtr lpBaseAddress, byte[] lpBuffer, nint nSize, out nint lpNumberOfBytesWritten);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool VirtualProtectEx(IntPtr hProcess, IntPtr lpAddress, nint dwSize, uint flNewProtect, out uint lpflOldProtect);
}
