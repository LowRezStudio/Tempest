using System.Runtime.InteropServices;
using System.Runtime.Loader;
using ConsoleAppFramework;
using Tempest.CLI.Launcher;
using Tempest.CLI.Marshal;
using Tempest.CLI.Server;
using Tempest.CLI.Build;
using Tempest.CLI.Rigby;
using Tempest.CLI.Mods;

// ponytail: register native DLL resolver to locate bundled Tauri resources (like blake3 or sqlite native libs) on all platforms.
AssemblyLoadContext.Default.ResolvingUnmanagedDll += (assembly, libraryName) =>
{
    if (!libraryName.Contains("blake3") && !libraryName.Contains("sqlite"))
    {
        return IntPtr.Zero;
    }

    var baseDir = AppContext.BaseDirectory;
    string filename;
    if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
    {
        filename = libraryName.EndsWith(".dll", StringComparison.OrdinalIgnoreCase) ? libraryName : $"{libraryName}.dll";
    }
    else if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX))
    {
        var baseName = libraryName.StartsWith("lib") ? libraryName : $"lib{libraryName}";
        filename = baseName.EndsWith(".dylib", StringComparison.OrdinalIgnoreCase) ? baseName : $"{baseName}.dylib";
    }
    else // Linux and others
    {
        var baseName = libraryName.StartsWith("lib") ? libraryName : $"lib{libraryName}";
        filename = baseName.EndsWith(".so", StringComparison.OrdinalIgnoreCase) ? baseName : $"{baseName}.so";
    }

    var searchDirs = new List<string> { baseDir };
    searchDirs.Add(Path.Combine(baseDir, "..", "Resources"));

    var parentDir = Path.GetDirectoryName(baseDir);
    if (parentDir != null)
    {
        searchDirs.Add(parentDir);
        var libDir = Path.Combine(parentDir, "lib");
        if (Directory.Exists(libDir))
        {
            searchDirs.Add(libDir);
            searchDirs.Add(Path.Combine(libDir, "tempest-launcher"));
            searchDirs.Add(Path.Combine(libDir, "tempest_launcher"));
        }
        var shareDir = Path.Combine(parentDir, "share");
        if (Directory.Exists(shareDir))
        {
            searchDirs.Add(shareDir);
            searchDirs.Add(Path.Combine(shareDir, "tempest-launcher"));
            searchDirs.Add(Path.Combine(shareDir, "tempest_launcher"));
        }
    }

    foreach (var dir in searchDirs)
    {
        var fullPath = Path.Combine(dir, filename);
        if (File.Exists(fullPath))
        {
            return NativeLibrary.Load(fullPath);
        }
    }

    return IntPtr.Zero;
};

var app = ConsoleApp.Create();

app.Add<LauncherCommands>();
app.Add<ServerCommands>("server");
app.Add<MarshalCommands>("marshal");
app.Add<BuildCommands>("build");
app.Add<RigbyCommands>("rigby");
app.Add<ModCommands>("mod");

if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
{
    app.Add<WineCommands>("wine");
}

app.Run(args);
