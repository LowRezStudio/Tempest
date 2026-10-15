#Requires -Version 7
# Native Windows build (MSVC) + the same four packages as scripts/package.sh.
param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$Out = "$PSScriptRoot/../dist"
)
$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $Root
try {
    cargo build --release --target $Target -p tempest-scripting
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    New-Item -ItemType Directory -Force -Path $Out | Out-Null
    $Out = Resolve-Path $Out
    Copy-Item "target/$Target/release/tempest_scripting.dll" "$Out/Tempest.Scripting64.dll" -Force
    $Luarc = '{ "runtime.version": "Lua 5.4", "workspace.library": ["types"], "workspace.checkThirdParty": false, "diagnostics.globals": ["MOD", "shared"] }'

    function Stage([string]$Name, [string]$Source, [bool]$Dll, [bool]$Types) {
        $S = Join-Path $Out $Name
        if (Test-Path $S) { Remove-Item $S -Recurse -Force }
        Copy-Item $Source $S -Recurse
        if ($Dll) { New-Item -ItemType Directory -Force -Path "$S/dlls" | Out-Null; Copy-Item "$Out/Tempest.Scripting64.dll" "$S/dlls/" }
        if ($Types) {
            New-Item -ItemType Directory -Force -Path "$S/types" | Out-Null
            Copy-Item "types/tempest.d.lua" "$S/types/"
            [System.IO.File]::WriteAllText((Join-Path $S ".luarc.json"), $Luarc)
        }
        python scripts/pack.py $S (Join-Path $Out "$Name.tempest")
        if ($LASTEXITCODE -ne 0) { throw "pack failed for $Name" }
        Write-Host "packed: $Out/$Name.tempest"
    }
    Stage "scripting" "packaging/scripting" $true $true
    Stage "console" "packaging/console" $false $false
    Stage "dev" "dev" $false $true
    Stage "lib" "examples/mods/tempest-scripting-lib" $false $false
}
finally {
    Pop-Location
}
