param([string]$Cli = (Join-Path $PSScriptRoot '..\Tempest.CLI\bin\Debug\net10.0\Tempest.CLI.dll'))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$testBase = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\out'))
$sandbox = Join-Path $testBase ('instance-copy-test-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $sandbox | Out-Null
function Invoke-Cli([string[]]$Arguments, [int]$Expected = 0) {
    $ErrorActionPreference = 'Continue'
    if ($Cli.EndsWith('.dll')) { $output = & dotnet exec $Cli @Arguments 2>&1 }
    else { $output = & $Cli @Arguments 2>&1 }
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne $Expected) { throw "CLI exit $LASTEXITCODE, expected ${Expected}: $output" }
    return $output
}
function Assert([bool]$Value, [string]$Message) { if (!$Value) { throw $Message } }
try {
    $original = Join-Path $sandbox 'Original'
    foreach ($dir in @('Binaries\Win64','Engine','ChaosGame\CookedPCConsole','ChaosGame\Config')) {
        New-Item -ItemType Directory -Path (Join-Path $original $dir) -Force | Out-Null
    }
    [IO.File]::WriteAllText((Join-Path $original 'Binaries\Win64\Paladins.exe'), 'fixture')
    [IO.File]::WriteAllText((Join-Path $original 'ChaosGame\CookedPCConsole\Skin_SF.upk'), 'vanilla-v1')
    [IO.File]::WriteAllText((Join-Path $original 'ChaosGame\CookedPCConsole\Other_SF.upk'), 'vanilla-v2')
    [IO.File]::WriteAllText((Join-Path $original 'ChaosGame\Config\Test.ini'), "[Test]`nValue=original`n")
    $loose = Join-Path $sandbox 'Skin_SF.upk'
    [IO.File]::WriteAllText($loose, 'mod-v1')
    $packageSource = Join-Path $sandbox 'mod'
    New-Item -ItemType Directory -Path (Join-Path $packageSource 'files\ChaosGame\CookedPCConsole'),(Join-Path $packageSource 'files\ChaosGame\Config') -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $packageSource 'manifest.toml'), "[mod]`nid = `"copy-test`"`nname = `"Copy Test`"`nversion = `"1.0.0`"`n")
    [IO.File]::WriteAllText((Join-Path $packageSource 'files\ChaosGame\CookedPCConsole\Other_SF.upk'), 'mod-v2')
    [IO.File]::WriteAllText((Join-Path $packageSource 'files\ChaosGame\Config\Test.ini'), "[Test]`nValue=modded`n")
    $package = Join-Path $sandbox 'fixture.tempest'
    [IO.Compression.ZipFile]::CreateFromDirectory($packageSource, $package)
    foreach ($mod in @($loose,$package)) {
        $result = (Invoke-Cli @('mod','install',$original,$mod,'--allow-unsigned','--json')) | ConvertFrom-Json
        Assert $result.Success 'Fixture mod installation failed'
    }
    $copy = Join-Path $sandbox 'Copy'
    $result = (Invoke-Cli @('instance','clone',$original,'--output',$copy)) | ConvertFrom-Json
    Assert ($result.Output -eq $copy) 'Clone returned wrong destination'
    $pathsJson = ConvertTo-Json -InputObject @($original,(Join-Path $original 'Binaries\Win64\Paladins.exe'),$copy) -Compress
    $encodedPaths = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($pathsJson))
    $roots = (Invoke-Cli @('instance','roots',$encodedPaths)) | ConvertFrom-Json
    Assert ($roots[0] -eq $roots[1] -and $roots[0] -ne $roots[2]) 'Canonical game roots are wrong'
    $records = (Invoke-Cli @('mod','list',$copy,'--json') | ConvertFrom-Json).Mods
    Assert ($records.Count -eq 2) 'Copy did not preserve both mods'
    foreach ($record in $records) {
        foreach ($file in @($record.InstalledFiles) + @($record.OwnedFiles)) {
            Assert ($file.StartsWith($copy + '\')) 'Copied mod still references original game'
        }
    }
    Invoke-Cli @('mod','remove',$copy,'Copy Test','--json') | Out-Null
    Invoke-Cli @('mod','remove',$copy,'Skin_SF.upk','--json') | Out-Null
    Assert ((Get-Content -LiteralPath (Join-Path $copy 'ChaosGame\CookedPCConsole\Other_SF.upk') -Raw) -eq 'vanilla-v2') 'Copied V2 backup was not restored'
    Assert ((Get-Content -LiteralPath (Join-Path $copy 'ChaosGame\CookedPCConsole\Skin_SF.upk') -Raw) -eq 'vanilla-v1') 'Copied V1 backup was not restored'
    Assert ((Get-Content -LiteralPath (Join-Path $copy 'ChaosGame\Config\Test.ini') -Raw).Contains('Value=original')) 'Copied INI backup was not restored'
    Assert ((Get-Content -LiteralPath (Join-Path $original 'ChaosGame\CookedPCConsole\Other_SF.upk') -Raw) -eq 'mod-v2') 'Removing copied mod changed original'
    [IO.File]::WriteAllText((Join-Path $original 'ChaosGame\CookedPCConsole\Skin_SF.upk'), 'updated-original')
    Assert ((Get-Content -LiteralPath (Join-Path $copy 'ChaosGame\CookedPCConsole\Skin_SF.upk') -Raw) -eq 'vanilla-v1') 'Copied file was linked to original'
    Invoke-Cli @('instance','clone',$original,'--output',$copy) 1 | Out-Null
    Invoke-Cli @('instance','clone',$original,'--output',(Join-Path $original 'nested')) 1 | Out-Null
    $junction = Join-Path $original 'linked'
    New-Item -ItemType Junction -Path $junction -Target $copy | Out-Null
    try {
        $failed = Join-Path $sandbox 'Failed'
        Invoke-Cli @('instance','clone',$original,'--output',$failed) 1 | Out-Null
        Assert (!(Test-Path -LiteralPath $failed)) 'Failed clone committed a destination'
        Assert (!(Get-ChildItem -LiteralPath $sandbox -Filter 'Failed.copy-*')) 'Failed clone left a staging folder'
    } finally { [IO.Directory]::Delete($junction) }
    'PASS: V1/V2 and INI snapshots, metadata rebasing, independent file updates, canonical roots, overwrite/overlap/link rejection and rollback'
} finally {
    $absolute = [IO.Path]::GetFullPath($sandbox)
    if (!$absolute.StartsWith($testBase + [IO.Path]::DirectorySeparatorChar)) { throw 'Invalid test cleanup path' }
    if (Test-Path -LiteralPath $absolute) { Remove-Item -LiteralPath $absolute -Recurse -Force }

}
