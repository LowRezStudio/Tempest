param([string]$Cli = (Join-Path $PSScriptRoot '..\Tempest.CLI\bin\Debug\net10.0\Tempest.CLI.dll'))
$ErrorActionPreference = 'Stop'
$testBase = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\out'))
$sandbox = Join-Path $testBase ('launch-sessions-test-' + [guid]::NewGuid())
$processes = @()
$gamePids = @()
try {
    $game = Join-Path $sandbox 'Game'
    $exe = Join-Path $game 'Binaries\Win64\Paladins.exe'
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($exe)),(Join-Path $game 'Engine') -Force | Out-Null
    Add-Type -OutputAssembly $exe -OutputType ConsoleApplication -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Threading;
public static class SessionGameFixture {
    public static void Main(string[] args) {
        Console.WriteLine(Process.GetCurrentProcess().Id + "|" + string.Join("|", args));
        while (true) Thread.Sleep(1000);
    }
}
'@
    for ($index = 0; $index -lt 2; $index++) {
        $start = New-Object Diagnostics.ProcessStartInfo
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardInput = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        if ($Cli.EndsWith('.dll')) { $start.FileName = 'dotnet'; $prefix = 'exec "' + [IO.Path]::GetFullPath($Cli) + '" ' }
        else { $start.FileName = [IO.Path]::GetFullPath($Cli); $prefix = '' }
        $start.Arguments = $prefix + 'launch "' + $game + '" --no-default-args -- -windowed'
        $client = [Diagnostics.Process]::Start($start)
        $processes += $client
        $line = $client.StandardOutput.ReadLineAsync()
        if (!$line.Wait(10000)) { throw 'Fake game did not start within ten seconds' }
        $parts = $line.Result.Split('|')
        if ($parts.Length -lt 2 -or $parts[0] -notmatch '^\d+$') { throw ('Unexpected game startup: ' + $line.Result + ' ' + $client.StandardError.ReadToEnd()) }
        $gamePids += [int]$parts[0]
        if ($parts -notcontains '-windowed') { throw 'Game arguments were not forwarded' }
    }
    if ($gamePids[0] -eq $gamePids[1]) { throw 'Launches reused the same game process' }
    # Wait through the launcher's startup/injection delay, then close only the first session.
    Start-Sleep -Milliseconds 1200
    $processes[0].StandardInput.WriteLine('kill')
    $processes[0].StandardInput.Flush()
    if (!$processes[0].WaitForExit(10000)) { throw 'First session did not stop' }
    if (Get-Process -Id $gamePids[0] -ErrorAction SilentlyContinue) { throw 'First game process was left running' }
    if (!(Get-Process -Id $gamePids[1] -ErrorAction SilentlyContinue) -or $processes[1].HasExited) { throw 'Stopping one session also stopped the other' }
    $processes[1].StandardInput.WriteLine('kill')
    $processes[1].StandardInput.Flush()
    if (!$processes[1].WaitForExit(10000)) { throw 'Second session did not stop' }
    if (Get-Process -Id $gamePids[1] -ErrorAction SilentlyContinue) { throw 'Second game process was left running' }
    Write-Output 'PASS: two real CLI/game process trees, individual stdin stops and game arguments'
} finally {
    foreach ($client in $processes) {
        try {
            if (!$client.HasExited) {
                $client.StandardInput.WriteLine('kill')
                $client.StandardInput.Flush()
                if (!$client.WaitForExit(5000)) { $client.Kill(); $client.WaitForExit() }
            }
        } finally { $client.Dispose() }
    }
    foreach ($gamePid in $gamePids) {
        $remaining = Get-Process -Id $gamePid -ErrorAction SilentlyContinue
        if ($remaining -and $remaining.Path -eq $exe) { $remaining.Kill(); $remaining.WaitForExit() }
    }
    $absolute = [IO.Path]::GetFullPath($sandbox)
    if (!$absolute.StartsWith($testBase + '\') -or [IO.Path]::GetFileName($absolute) -notlike 'launch-sessions-test-*') { throw 'Invalid session fixture cleanup path' }
    if (Test-Path -LiteralPath $absolute) { Remove-Item -LiteralPath $absolute -Recurse -Force }
}
