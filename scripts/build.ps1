param(
    [ValidateSet('build','test','fmt')][string]$Action = 'build',
    [string]$ToolchainBin = '',
    [string]$RustToolchain = 'stable-x86_64-pc-windows-gnu',
    [string]$TestFilter = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
if ($ToolchainBin) {
    $ToolchainBin = (Resolve-Path -LiteralPath $ToolchainBin).Path
    $env:PATH = $ToolchainBin + ';' + $env:PATH
}
if ($Action -ne 'fmt') {
    $null = Get-Command windres -ErrorAction Stop
    $linker = Get-Command x86_64-w64-mingw32-gcc -ErrorAction Stop
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = $linker.Source
    $env:RUSTFLAGS = '-C link-self-contained=yes'
    if (-not $env:CARGO_TARGET_DIR -and ($root -match '[^\x00-\x7F]')) {
        $env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'cargo_target_minecraft_pet'
    }
}
Push-Location (Join-Path $root 'native')
try {
    if ($Action -eq 'fmt') {
        cargo "+$RustToolchain" fmt -- --check
    } elseif ($Action -eq 'test') {
        if ($TestFilter) {
            cargo "+$RustToolchain" test --locked -- $TestFilter --nocapture
        } else {
            cargo "+$RustToolchain" test --locked
        }
    } else {
        cargo "+$RustToolchain" build --locked --release
        if ($LASTEXITCODE -eq 0 -and $env:CARGO_TARGET_DIR) {
            $destDir = Join-Path $root 'native\target\release'
            New-Item -ItemType Directory -Force -Path $destDir | Out-Null
            $builtExe = Join-Path $env:CARGO_TARGET_DIR 'release\minecraft-pet.exe'
            if (Test-Path $builtExe) {
                Copy-Item $builtExe (Join-Path $destDir 'minecraft-pet.exe') -Force
            }
        }
    }
    if ($LASTEXITCODE -ne 0) { throw "Cargo $Action failed" }
} finally { Pop-Location }
