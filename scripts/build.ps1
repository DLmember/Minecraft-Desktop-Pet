param(
    [ValidateSet('build','test','fmt')][string]$Action = 'build',
    [string]$ToolchainBin = '',
    [string]$RustToolchain = 'stable-x86_64-pc-windows-gnu'
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
}
Push-Location (Join-Path $root 'native')
try {
    if ($Action -eq 'fmt') {
        cargo "+$RustToolchain" fmt -- --check
    } elseif ($Action -eq 'test') {
        cargo "+$RustToolchain" test --locked
    } else {
        cargo "+$RustToolchain" build --locked --release
    }
    if ($LASTEXITCODE -ne 0) { throw "Cargo $Action failed" }
} finally { Pop-Location }
