param([string]$Cli = (Join-Path $PSScriptRoot '..\Tempest.CLI\bin\Debug\net10.0\Tempest.CLI.dll'))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$sandbox = Join-Path ([IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\out'))) ('cleanup-test-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $sandbox | Out-Null
function Invoke-Cli([string[]]$Arguments, [int]$Expected = 0) {
    if ($Arguments[0] -eq 'cleanup' -and $Arguments -contains '--confirm') { $Arguments += '--launcher-session' }
    $ErrorActionPreference = 'Continue'
    $output = & dotnet exec $Cli @Arguments 2>&1
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne $Expected) { throw "CLI exit $LASTEXITCODE, expected ${Expected}: $output" }
    return $output
}
function Game([string]$Name) {
    $root = Join-Path $sandbox $Name
    foreach ($dir in @('Binaries\Win64', 'Engine', 'ChaosGame\CookedPCConsole', 'ChaosGame\Config')) {
        New-Item -ItemType Directory -Path (Join-Path $root $dir) -Force | Out-Null
    }
    [IO.File]::WriteAllText((Join-Path $root 'Binaries\Win64\Paladins.exe'), 'fixture')
    [IO.File]::WriteAllText((Join-Path $root 'ChaosGame\CookedPCConsole\Skin_SF.upk'), 'vanilla-v1')
    [IO.File]::WriteAllText((Join-Path $root 'ChaosGame\CookedPCConsole\Other_SF.upk'), 'vanilla-v2')
    [IO.File]::WriteAllText((Join-Path $root 'ChaosGame\Config\Test.ini'), "[Test]`nValue=original`nKeep=yes`n")
    return $root
}
function Entry([string]$Root, [bool]$Managed = $false) {
    return @{ id = [guid]::NewGuid().ToString(); label = [IO.Path]::GetFileName($Root); path = $Root; origin = $(if ($Managed) {'download'} else {'import'}); managedPath = $(if ($Managed) {$Root} else {$null}) }
}
function Manifest([object[]]$Entries, [string]$Name = 'registry.json') {
    $target = Join-Path $sandbox $Name
    @{ version = 1; cleaned = $false; instances = $Entries } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $target -Encoding UTF8
    return $target
}
function Assert([bool]$Value, [string]$Message) { if (!$Value) { throw $Message } }
try {
    $imported = Game 'Imported'
    $managed = Game 'Managed'
    $loose = Join-Path $sandbox 'Skin_SF.upk'
    [IO.File]::WriteAllText($loose, 'mod-v1')
    $source = Join-Path $sandbox 'v2'
    New-Item -ItemType Directory -Path (Join-Path $source 'files\ChaosGame\CookedPCConsole'),(Join-Path $source 'files\ChaosGame\Config') -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $source 'manifest.toml'), "[mod]`nid = `"cleanup-v2`"`nname = `"Cleanup V2`"`nversion = `"1.0.0`"`n")
    [IO.File]::WriteAllText((Join-Path $source 'files\ChaosGame\CookedPCConsole\Other_SF.upk'), 'mod-v2')
    [IO.File]::WriteAllText((Join-Path $source 'files\ChaosGame\Config\Test.ini'), "[Test]`nValue=modded`n")
    $package = Join-Path $sandbox 'fixture.tempest'
    [IO.Compression.ZipFile]::CreateFromDirectory($source, $package)
    foreach ($root in @($imported, $managed)) {
        $result = (Invoke-Cli @('mod','install',$root,$loose,'--allow-unsigned','--json')) | ConvertFrom-Json
        Assert $result.Success 'V1 installation failed'
        $result = (Invoke-Cli @('mod','install',$root,$package,'--allow-unsigned','--json')) | ConvertFrom-Json
        Assert $result.Success 'V2 installation failed'
    }
    $result = (Invoke-Cli @('mod','disable',$imported,'Skin_SF.upk','--json')) | ConvertFrom-Json
    Assert $result.Success 'V1 disable failed'
    # Duplicated registrations must preserve an imported folder even if one says managed.
    $registry = Manifest @((Entry $imported), (Entry $imported $true), (Entry $managed $true))
    Invoke-Cli @('cleanup','run','--manifest',$registry,'--dry-run') | Out-Null
    Assert (Test-Path -LiteralPath $managed) 'Dry run deleted a folder'
    Invoke-Cli @('cleanup','run','--manifest',$registry,'--confirm') | Out-Null
    Assert (!(Test-Path -LiteralPath $managed)) 'Managed game was not deleted'
    Assert ((Get-Content -LiteralPath (Join-Path $imported 'ChaosGame\CookedPCConsole\Skin_SF.upk') -Raw) -eq 'vanilla-v1') 'Disabled V1 removal changed vanilla data'
    Assert ((Get-Content -LiteralPath (Join-Path $imported 'ChaosGame\CookedPCConsole\Other_SF.upk') -Raw) -eq 'vanilla-v2') 'V2 backup was not restored'
    $ini = Get-Content -LiteralPath (Join-Path $imported 'ChaosGame\Config\Test.ini') -Raw
    Assert ($ini.Contains('Value=original') -and $ini.Contains('Keep=yes') -and !$ini.Contains('modded')) 'INI was not restored'
    Assert (!(Test-Path -LiteralPath (Join-Path $imported '.tempest'))) 'Mod data was left behind'
    $state = Get-Content -LiteralPath $registry -Raw | ConvertFrom-Json
    Assert ($state.cleaned -and $state.instances.Count -eq 0 -and $state.removedIds.Count -eq 3) 'Registry completion failed'
    $locked = Game 'Locked'
    Invoke-Cli @('mod','install',$locked,$package,'--allow-unsigned','--json') | Out-Null
    $lockedRegistry = Manifest @((Entry $locked)) 'locked.json'
    $handle = [IO.File]::Open((Join-Path $locked 'ChaosGame\CookedPCConsole\Other_SF.upk'), [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try { Invoke-Cli @('cleanup','run','--manifest',$lockedRegistry,'--confirm') 1 | Out-Null }
    finally { $handle.Dispose() }
    $state = Get-Content -LiteralPath $lockedRegistry -Raw | ConvertFrom-Json
    Assert (!$state.cleaned -and $state.instances.Count -eq 1) 'Failed cleanup discarded its instance'
    Invoke-Cli @('cleanup','run','--manifest',$lockedRegistry,'--confirm') | Out-Null
    Assert ((Get-Content -LiteralPath (Join-Path $locked 'ChaosGame\CookedPCConsole\Other_SF.upk') -Raw) -eq 'vanilla-v2') 'Retry lost original data'
    $unsafeRegistry = Manifest @((Entry ([IO.Path]::GetPathRoot($sandbox)) $true)) 'unsafe.json'
    Invoke-Cli @('cleanup','run','--manifest',$unsafeRegistry,'--dry-run') 1 | Out-Null
    $broken = Game 'Broken'
    New-Item -ItemType Directory -Path (Join-Path $broken '.tempest\mods') -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $broken '.tempest\mods\mods.json'), 'invalid json')
    $brokenRegistry = Manifest @((Entry $broken $true)) 'broken.json'
    Invoke-Cli @('cleanup','run','--manifest',$brokenRegistry,'--confirm') 1 | Out-Null
    Assert (Test-Path -LiteralPath $broken) 'Malformed metadata permitted deletion'
    $escaped = Game 'Escaped'
    $outside = Join-Path $sandbox 'outside.txt'
    [IO.File]::WriteAllText($outside, 'preserve')
    New-Item -ItemType Directory -Path (Join-Path $escaped '.tempest\mods') -Force | Out-Null
    ConvertTo-Json -InputObject @(@{Id='escape';Name='Escape';Kind='V2';InstalledFiles=@($outside);OwnedFiles=@($outside);MetadataVersion=2;Enabled=$true}) -Depth 5 | Set-Content -LiteralPath (Join-Path $escaped '.tempest\mods\mods.json') -Encoding UTF8
    $escapeRegistry = Manifest @((Entry $escaped)) 'escape.json'
    Invoke-Cli @('cleanup','run','--manifest',$escapeRegistry,'--dry-run') 1 | Out-Null
    Assert ((Get-Content -LiteralPath $outside -Raw) -eq 'preserve') 'External mod path was touched'
    $junction = Join-Path $sandbox 'Junction'
    New-Item -ItemType Junction -Path $junction -Target $imported | Out-Null
    try {
        $linkRegistry = Manifest @((Entry $junction $true)) 'link.json'
        Invoke-Cli @('cleanup','run','--manifest',$linkRegistry,'--dry-run') 1 | Out-Null
        Assert (Test-Path -LiteralPath $imported) 'Junction target was touched'
    } finally { [IO.Directory]::Delete($junction) }
    $parent = Game 'Parent'
    $child = Join-Path $parent 'Child'
    foreach ($dir in @('Binaries', 'Engine')) { New-Item -ItemType Directory -Path (Join-Path $child $dir) -Force | Out-Null }
    $overlapRegistry = Manifest @((Entry $parent $true), (Entry $child)) 'overlap.json'
    Invoke-Cli @('cleanup','run','--manifest',$overlapRegistry,'--confirm') 1 | Out-Null
    Assert (Test-Path -LiteralPath $child) 'Nested imported instance was touched'
    'PASS: imported files, V1/V2 and INI restoration, disabled mods, managed deletion, duplicate paths, dry run, retry, and invalid roots/metadata'
    'PASS: escaping mod paths, junctions, and overlapping instances rejected'
} finally {
    $resolved = [IO.Path]::GetFullPath($sandbox)
    $allowed = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\out')) + [IO.Path]::DirectorySeparatorChar
    if (!$resolved.StartsWith($allowed, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe fixture cleanup target' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
