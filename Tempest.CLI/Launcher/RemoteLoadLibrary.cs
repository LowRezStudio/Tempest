using System.Runtime.InteropServices;
using System.Text;

namespace Tempest.CLI.Launcher;

/// <summary>
/// Loads a DLL into a running game on native Windows: the CLI starts a thread at <c>LoadLibraryW</c>
/// in the game itself, rather than through a small injector exe, which Defender's heuristics take for a
/// trojan. Under Wine the game is not a process the CLI can open, so it still runs the one in inject/.
/// </summary>
internal static class RemoteLoadLibrary
{
    /// <summary>Whether <paramref name="dllPath"/> was loaded; prints why not otherwise.</summary>
    public static async Task<bool> InjectAsync(int pid, string dllPath, bool is64Bit)
    {
        var process = OpenProcess(ProcessCreateThread | ProcessQueryInformation | ProcessVmOperation | ProcessVmWrite | ProcessVmRead, false, pid);
        if (process == IntPtr.Zero)
        {
            Console.Error.WriteLine($"[inject] could not open the game (PID {pid}, error {System.Runtime.InteropServices.Marshal.GetLastWin32Error()})");
            return false;
        }
        try
        {
            var loadLibrary = await LoadLibraryAddressAsync(process, is64Bit);
            if (loadLibrary == IntPtr.Zero)
                return false;

            var path = Encoding.Unicode.GetBytes(dllPath + '\0');
            var remote = VirtualAllocEx(process, IntPtr.Zero, path.Length, MemCommit | MemReserve, PageReadWrite);
            if (remote == IntPtr.Zero)
            {
                Console.Error.WriteLine($"[inject] could not allocate memory in the game (error {System.Runtime.InteropServices.Marshal.GetLastWin32Error()})");
                return false;
            }
            try
            {
                if (!WriteProcessMemory(process, remote, path, path.Length, out _))
                {
                    Console.Error.WriteLine($"[inject] could not write the DLL path into the game (error {System.Runtime.InteropServices.Marshal.GetLastWin32Error()})");
                    return false;
                }
                var thread = CreateRemoteThread(process, IntPtr.Zero, 0, loadLibrary, remote, 0, IntPtr.Zero);
                if (thread == IntPtr.Zero)
                {
                    Console.Error.WriteLine($"[inject] could not start a thread in the game (error {System.Runtime.InteropServices.Marshal.GetLastWin32Error()})");
                    return false;
                }
                try
                {
                    await Task.Run(() => WaitForSingleObject(thread, Infinite));
                    // The thread's exit code is the low half of the module handle, 0 when LoadLibraryW failed.
                    if (!GetExitCodeThread(thread, out var module) || module == 0)
                    {
                        Console.Error.WriteLine("[inject] LoadLibraryW failed in the game");
                        return false;
                    }
                    Console.Error.WriteLine($"[inject] loaded (module handle: 0x{module:x})");
                    return true;
                }
                finally { CloseHandle(thread); }
            }
            finally { VirtualFreeEx(process, remote, 0, MemRelease); }
        }
        finally { CloseHandle(process); }
    }

    /// <summary>
    /// <c>LoadLibraryW</c> in the game. kernel32 sits at the same address in every process of a boot
    /// for one architecture, so ours serves when the game matches the CLI; for the Win32 client under
    /// a Win64 CLI it is the WOW64 kernel32's export, at wherever that is loaded in the game.
    /// </summary>
    private static async Task<IntPtr> LoadLibraryAddressAsync(IntPtr process, bool is64Bit)
    {
        var architecture = RuntimeInformation.ProcessArchitecture;
        if (architecture == (is64Bit ? Architecture.X64 : Architecture.X86))
            return NativeLibrary.GetExport(NativeLibrary.Load("kernel32.dll"), "LoadLibraryW");
        if (is64Bit || architecture != Architecture.X64)
        {
            Console.Error.WriteLine($"[inject] a {architecture} launcher cannot load DLLs into the {(is64Bit ? "Win64" : "Win32")} client");
            return IntPtr.Zero;
        }

        var kernel32 = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.SystemX86), "kernel32.dll");
        var rva = ExportRva(kernel32, "LoadLibraryW");
        if (rva == 0)
        {
            Console.Error.WriteLine($"[inject] LoadLibraryW is not exported by {kernel32}");
            return IntPtr.Zero;
        }
        // A game started a moment ago may not have mapped its 32-bit kernel32 yet.
        for (var i = 0; i < 100; i++)
        {
            if (Wow64ModuleBase(process, "kernel32.dll") is { } module)
                return module + (nint)rva;
            await Task.Delay(100);
        }
        Console.Error.WriteLine("[inject] could not find kernel32.dll in the Win32 client");
        return IntPtr.Zero;
    }

    private static IntPtr? Wow64ModuleBase(IntPtr process, string name)
    {
        var modules = new IntPtr[1024];
        if (!EnumProcessModulesEx(process, modules, (uint)(modules.Length * IntPtr.Size), out var needed, ListModules32Bit))
            return null;
        var buffer = new char[260];
        foreach (var module in modules.AsSpan(0, (int)Math.Min(needed / (uint)IntPtr.Size, (uint)modules.Length)))
        {
            var length = GetModuleBaseNameW(process, module, buffer, (uint)buffer.Length);
            if (length > 0 && new string(buffer, 0, (int)length).Equals(name, StringComparison.OrdinalIgnoreCase))
                return module;
        }
        return null;
    }

    /// <summary>The RVA <paramref name="dll"/> exports <paramref name="name"/> at; 0 when it doesn't, or forwards it.</summary>
    private static uint ExportRva(string dll, string name)
    {
        var image = File.ReadAllBytes(dll);
        uint U32(long at) => BitConverter.ToUInt32(image, (int)at);
        ushort U16(long at) => BitConverter.ToUInt16(image, (int)at);

        var pe = U32(0x3C);
        if (U32(pe) != 0x00004550) return 0; // "PE\0\0"
        var sectionCount = U16(pe + 6);
        var optionalHeader = pe + 24;
        var dataDirectories = optionalHeader + (U16(optionalHeader) == 0x20B ? 112 : 96);
        var exportRva = U32(dataDirectories);
        var exportSize = U32(dataDirectories + 4);
        if (exportRva == 0) return 0;
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
            throw new InvalidDataException($"RVA 0x{rva:x} lies outside every section of {dll}");
        }

        var exports = Offset(exportRva);
        var nameCount = U32(exports + 24);
        var functions = Offset(U32(exports + 28));
        var names = Offset(U32(exports + 32));
        var ordinals = Offset(U32(exports + 36));
        var wanted = Encoding.ASCII.GetBytes(name + '\0');
        for (var i = 0; i < nameCount; i++)
        {
            var at = (int)Offset(U32(names + i * 4));
            if (!image.AsSpan(at, Math.Min(wanted.Length, image.Length - at)).SequenceEqual(wanted)) continue;
            var rva = U32(functions + U16(ordinals + i * 2) * 4);
            // An RVA inside the export directory is a forwarder string, not code.
            return rva >= exportRva && rva < exportRva + exportSize ? 0 : rva;
        }
        return 0;
    }

    private const uint ProcessCreateThread = 0x0002;
    private const uint ProcessVmOperation = 0x0008;
    private const uint ProcessVmRead = 0x0010;
    private const uint ProcessVmWrite = 0x0020;
    private const uint ProcessQueryInformation = 0x0400;
    private const uint MemCommit = 0x1000;
    private const uint MemReserve = 0x2000;
    private const uint MemRelease = 0x8000;
    private const uint PageReadWrite = 0x04;
    private const uint Infinite = 0xFFFFFFFF;
    private const uint ListModules32Bit = 0x01;

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr OpenProcess(uint dwDesiredAccess, bool bInheritHandle, int dwProcessId);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr VirtualAllocEx(IntPtr hProcess, IntPtr lpAddress, nint dwSize, uint flAllocationType, uint flProtect);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool VirtualFreeEx(IntPtr hProcess, IntPtr lpAddress, nint dwSize, uint dwFreeType);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool WriteProcessMemory(IntPtr hProcess, IntPtr lpBaseAddress, byte[] lpBuffer, nint nSize, out nint lpNumberOfBytesWritten);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr CreateRemoteThread(IntPtr hProcess, IntPtr lpThreadAttributes, nint dwStackSize, IntPtr lpStartAddress, IntPtr lpParameter, uint dwCreationFlags, IntPtr lpThreadId);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint WaitForSingleObject(IntPtr hHandle, uint dwMilliseconds);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetExitCodeThread(IntPtr hThread, out uint lpExitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(IntPtr hObject);

    [DllImport("psapi.dll", SetLastError = true)]
    private static extern bool EnumProcessModulesEx(IntPtr hProcess, [Out] IntPtr[] lphModule, uint cb, out uint lpcbNeeded, uint dwFilterFlag);

    [DllImport("psapi.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern uint GetModuleBaseNameW(IntPtr hProcess, IntPtr hModule, [Out] char[] lpBaseName, uint nSize);
}
