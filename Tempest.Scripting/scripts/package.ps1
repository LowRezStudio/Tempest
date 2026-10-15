#Requires -Version 7
# Native Windows build (MSVC) + stage dist/ exactly like scripts/package.sh.
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

    $Stage = Join-Path $Out "testmod"
    if (Test-Path $Stage) { Remove-Item $Stage -Recurse -Force }
    New-Item -ItemType Directory -Force -Path "$Stage/dlls", "$Stage/scripts" | Out-Null
    Copy-Item "$Out/Tempest.Scripting64.dll" "$Stage/dlls/" -Force
    Copy-Item "examples/*.lua" "$Stage/scripts/" -Force
    Copy-Item "packaging/manifest.toml" "$Stage/manifest.toml" -Force

    # CHECKSUMS: "<sha256> <relpath>", ordinal order, LF-joined, no trailing
    # newline — what Tempest.CLI `mod pack` writes.
    [string[]]$files = Get-ChildItem $Stage -Recurse -File |
        Where-Object { $_.Name -notin @("CHECKSUMS", "CHECKSUMS.asc") } |
        ForEach-Object { $_.FullName.Substring($Stage.Length + 1).Replace('\', '/') }
    [Array]::Sort($files, [System.StringComparer]::Ordinal)
    $lines = foreach ($rel in $files) {
        $hash = (Get-FileHash (Join-Path $Stage $rel) -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash $rel"
    }
    [System.IO.File]::WriteAllText((Join-Path $Stage "CHECKSUMS"), ($lines -join "`n"))

    $Zip = Join-Path $Out "testmod.tempest"
    if (Test-Path $Zip) { Remove-Item $Zip -Force }
    Compress-Archive -Path (Join-Path $Stage "*") -DestinationPath $Zip -CompressionLevel Optimal
    Write-Host "staged: $Out/Tempest.Scripting64.dll"
    Write-Host "packed: $Zip"
}
finally {
    Pop-Location
}
