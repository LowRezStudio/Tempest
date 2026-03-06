using System.Runtime.InteropServices;
using ConsoleAppFramework;
using Tempest.CLI.Launcher;
using Tempest.CLI.Marshal;
using Tempest.CLI.Server;
using Tempest.CLI.Build;
using Tempest.CLI.Rigby;

var app = ConsoleApp.Create();

app.Add<LauncherCommands>();
app.Add<ServerCommands>("server");
app.Add<MarshalCommands>("marshal");
app.Add<BuildCommands>("build");
app.Add<RigbyCommands>("rigby");

if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
{
    app.Add<WineCommands>("wine");
}

app.Run(args);
