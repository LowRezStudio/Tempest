#Requires -Version 7
# Native Windows build (MSVC) + the same four packages as scripts/package.sh.
# Builds the Win64 DLL and, unless -Targets is narrowed, the Win32 one too
# (`rustup target add i686-pc-windows-msvc`).
param(
    [string[]]$Targets = @("x86_64-pc-windows-msvc", "i686-pc-windows-msvc"),
    [string]$Out = "$PSScriptRoot/../dist"
)
$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $Root
try {
    New-Item -ItemType Directory -Force -Path $Out | Out-Null
    $Out = Resolve-Path $Out
    # (bits, dll) pairs; the runtime package stages each DLL under dlls/<bits>/.
    $Dlls = @()
    foreach ($Target in $Targets) {
        cargo build --release --target $Target -p tempest-scripting
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $Target" }
        $Bits = if ($Target -like "i686-*") { "32" } else { "64" }
        Copy-Item "target/$Target/release/tempest_scripting.dll" "$Out/Tempest.Scripting$Bits.dll" -Force
        $Dlls += ,@($Bits, "$Out/Tempest.Scripting$Bits.dll")
    }
    $Luarc = '{ "runtime.version": "LuaJIT", "workspace.library": ["types"], "workspace.checkThirdParty": false, "diagnostics.globals": ["MOD", "shared"] }'

    function Stage([string]$Name, [string]$Source, [bool]$Dll, [bool]$Types) {
        $S = Join-Path $Out $Name
        if (Test-Path $S) { Remove-Item $S -Recurse -Force }
        Copy-Item $Source $S -Recurse
        if ($Dll) { foreach ($D in $Dlls) { New-Item -ItemType Directory -Force -Path "$S/dlls/$($D[0])" | Out-Null; Copy-Item $D[1] "$S/dlls/$($D[0])/" } }
        if ($Types) {
            New-Item -ItemType Directory -Force -Path "$S/types" | Out-Null
            Copy-Item "types/tempest.d.lua", "types/imgui.d.lua" "$S/types/"
            [System.IO.File]::WriteAllText((Join-Path $S ".luarc.json"), $Luarc)
        }
        python scripts/pack.py $S (Join-Path $Out "$Name.tempest")
        if ($LASTEXITCODE -ne 0) { throw "pack failed for $Name" }
        Write-Host "packed: $Out/$Name.tempest"
    }
    Stage "scripting" "mods/scripting" $true $true
    Stage "console" "mods/console" $false $false
    Stage "dev" "mods/dev" $false $true
    Stage "lib" "mods/lib" $false $false
}
finally {
    Pop-Location
}
