param(
    [switch]$TestOnly,
    [switch]$Window,
    [string]$SavePng = ""
)

$ErrorActionPreference = "Stop"
$WorkspaceRoot = Split-Path -Parent $PSScriptRoot
Set-Location $WorkspaceRoot

if ($SavePng -ne "") {
    Write-Host "[iOS App] Exporting 393x852 iPhone screenshot to $SavePng..." -ForegroundColor Cyan
    cargo run -p ios_app --bin ios_app_cli --features winit -- --save-png $SavePng
    if ($LASTEXITCODE -ne 0) { throw "cargo run --save-png failed" }
    exit 0
}

if ($Window) {
    Write-Host "[iOS App] Launching interactive 393x852 iPhone Simulator Window..." -ForegroundColor Green
    cargo run -p ios_app --bin ios_app_cli --features winit -- --window
    if ($LASTEXITCODE -ne 0) { throw "cargo run --window failed" }
    exit 0
}

Write-Host "[1/2] Running iOS App integration tests (Dynamic Island SafeArea, UITouchPhase, Swipe-Back, UIKeyboard, Retina @3x, Rotation)..." -ForegroundColor Cyan
cargo test -p ios_app
if ($LASTEXITCODE -ne 0) { throw "cargo test -p ios_app failed" }

if ($TestOnly) {
    exit 0
}

Write-Host "[2/2] Emitting iOS host glue (velox_ios.h / VeloxMetalView.swift / Info.plist) and verifying CADisplayLink lifecycle..." -ForegroundColor Cyan
$IosHostDir = Join-Path $WorkspaceRoot "target\ios_host_build"
cargo run -p ios_app --bin ios_app_cli -- --emit-host $IosHostDir
if ($LASTEXITCODE -ne 0) { throw "Failed to emit iOS host files" }
cargo run -p ios_app --bin ios_app_cli
if ($LASTEXITCODE -ne 0) { throw "cargo run -p ios_app failed" }
