param([switch]$SkipTests)
$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot 'native')
try {
    if (-not $SkipTests) {
        cargo test --bin minecraft-pet --locked
        if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
    }
    cargo build --release --bin minecraft-pet --locked
    if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
    $outDir = Join-Path $PSScriptRoot 'dist'
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    Copy-Item -LiteralPath 'target/release/minecraft-pet.exe' -Destination (Join-Path $outDir 'Minecraft桌宠-Blockbench兼容版.exe')
} finally { Pop-Location }
