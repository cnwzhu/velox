param(
    [switch]$TestOnly,
    [switch]$Window,
    [string]$SavePng = ""
)

$ErrorActionPreference = "Stop"
$WorkspaceRoot = Split-Path -Parent $PSScriptRoot
Set-Location $WorkspaceRoot

if ($SavePng -ne "") {
    Write-Host "[HarmonyOS App] Exporting 384x832 HarmonyOS NEXT phone screenshot to $SavePng..." -ForegroundColor Cyan
    cargo run -p harmony_app --bin harmony_app_cli --features winit -- --save-png $SavePng
    if ($LASTEXITCODE -ne 0) { throw "cargo run --save-png failed" }
    exit 0
}

if ($Window) {
    Write-Host "[HarmonyOS App] Launching interactive 384x832 HarmonyOS NEXT Simulator Window..." -ForegroundColor Green
    cargo run -p harmony_app --bin harmony_app_cli --features winit -- --window
    if ($LASTEXITCODE -ne 0) { throw "cargo run --window failed" }
    exit 0
}

Write-Host "[1/2] Running HarmonyOS NEXT App integration tests (AvoidArea, OH_NativeXComponent Touch, Swipe-Back, TYPE_KEYBOARD, onBackPress, Foldable Rotation)..." -ForegroundColor Cyan
cargo test -p harmony_app
if ($LASTEXITCODE -ne 0) { throw "cargo test -p harmony_app failed" }

if ($TestOnly) {
    exit 0
}

Write-Host "[2/2] Emitting HarmonyOS NEXT Stage host glue (module.json5 / Index.ets / EntryAbility.ets / index.d.ts) and verifying OH_NativeXComponent lifecycle..." -ForegroundColor Cyan
$HarmonyHostDir = Join-Path $WorkspaceRoot "target\harmony_host_build"
cargo run -p harmony_app --bin harmony_app_cli -- --emit-host $HarmonyHostDir
if ($LASTEXITCODE -ne 0) { throw "Failed to emit HarmonyOS host files" }
cargo run -p harmony_app --bin harmony_app_cli
if ($LASTEXITCODE -ne 0) { throw "cargo run -p harmony_app failed" }
