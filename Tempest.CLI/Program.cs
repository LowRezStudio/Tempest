using System.Runtime.InteropServices;
using ConsoleAppFramework;
using Tempest.CLI.Launcher;
using Tempest.CLI.Marshal;
using Tempest.CLI.Server;

var app = ConsoleApp.Create();

app.Add<LauncherCommands>();
app.Add<ServerCommands>("server");
app.Add<MarshalCommands>("marshal");

if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
{
    app.Add<WineCommands>("wine");
}

app.Run(args);