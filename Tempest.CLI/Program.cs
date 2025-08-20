using System.Runtime.InteropServices;
using ConsoleAppFramework;
using Tempest.CLI;

var app = ConsoleApp.Create();

app.Add<LauncherCommands>();
app.Add<ServerCommands>("server");
app.Add<MarshalCommands>("marshal");
app.Add<ProjectCommands>("project");
app.Add<UtilityCommands>();

if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
{
    app.Add<WineCommands>("wine");
}

app.Run(args);