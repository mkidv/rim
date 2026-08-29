# PowerShell script to update RIM WASM & payload assets in mki.dev
param(
    [string]$WebRepo = "D:\Development\web\mki.dev"
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RimRoot = Split-Path -Parent $ScriptDir

Write-Host "=== Updating RIM Web Assets ===" -ForegroundColor Cyan
Write-Host "RIM Root: $RimRoot"
Write-Host "Web Root: $WebRepo"


# 1. Build release WASM binary
Write-Host "`n[1/3] Building release WebAssembly module..." -ForegroundColor Yellow
Push-Location $RimRoot
try {
    cargo build -p wasm-synth --target wasm32-unknown-unknown --release
} finally {
    Pop-Location
}

# 2. Verify targets
$WasmSrc = Join-Path $RimRoot "target\wasm32-unknown-unknown\release\wasm_synth.wasm"
$PayloadSrc = Join-Path $RimRoot "examples\wasm-synth\payload\alpine_payload.tar"
$WasmDest = Join-Path $WebRepo "public\rim\wasm_synth.wasm"
$PayloadDest = Join-Path $WebRepo "public\rim\payload\alpine_payload.tar"

if (-not (Test-Path $WasmSrc)) {
    throw "WASM artifact not found at $WasmSrc"
}
if (-not (Test-Path $PayloadSrc)) {
    throw "Alpine payload TAR not found at $PayloadSrc"
}

# 3. Copy to mki.dev
Write-Host "`n[2/3] Copying artifacts to mki.dev/public/rim/..." -ForegroundColor Yellow
$PayloadDestDir = Split-Path -Parent $PayloadDest
if (-not (Test-Path $PayloadDestDir)) {
    New-Item -ItemType Directory -Force -Path $PayloadDestDir | Out-Null
}

Copy-Item $WasmSrc $WasmDest -Force
Copy-Item $PayloadSrc $PayloadDest -Force

Write-Host "`n[3/3] Verifying copied artifacts:" -ForegroundColor Green
Get-Item $WasmDest, $PayloadDest | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize

Write-Host "=== Web assets successfully updated! ===" -ForegroundColor Green
