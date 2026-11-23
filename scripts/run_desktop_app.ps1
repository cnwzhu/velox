param(
    [switch]$TestOnly,
    [switch]$Window,
    [switch]$Software,
    [switch]$Light,
    [string]$SavePng = ""
)

$ErrorActionPreference = "Stop"
$WorkspaceRoot = Split-Path -Parent $PSScriptRoot
Set-Location $WorkspaceRoot

$AppArgs = @()
if ($Light) {
    $AppArgs += "--light"
}
if ($Software) {
    $AppArgs += "--software"
}

if ($SavePng -ne "") {
    Write-Host "[Desktop App] Exporting 960x640 Desktop Studio screenshot to $SavePng..." -ForegroundColor Cyan
    cargo run -p desktop_app -- --scale 1 --save-png $SavePng @AppArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo run --save-png failed" }
    exit 0
}

if ($Window) {
    Write-Host "[Desktop App] Launching interactive 960x640 Desktop Workbench Window..." -ForegroundColor Green
    cargo run -p desktop_app -- --window @AppArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo run --window failed" }
    exit 0
}

Write-Host "[1/2] Running Desktop App integration tests (Winit, Pure Wayland, Theme, Modal Occlusion, CodeEditor, Table, Resize)..." -ForegroundColor Cyan
cargo test -p desktop_app
if ($LASTEXITCODE -ne 0) { throw "cargo test -p desktop_app failed" }

if ($TestOnly) {
    exit 0
}

Write-Host "[2/2] Running Headless Winit/Wayland check and generating PNG preview snapshot..." -ForegroundColor Cyan
cargo run -p desktop_app -- --headless @AppArgs
if ($LASTEXITCODE -ne 0) { throw "cargo run --headless failed" }

$PreviewPath = Join-Path $WorkspaceRoot "target\desktop_app_preview.png"
cargo run -p desktop_app -- --scale 1 --save-png $PreviewPath @AppArgs
if ($LASTEXITCODE -ne 0) { throw "cargo run --save-png failed" }

if (Test-Path $PreviewPath) {
    $fileInfo = Get-Item $PreviewPath
    if ($fileInfo.Length -le 0) {
        throw "Generated PNG snapshot is empty: $PreviewPath"
    }
    Write-Host "      -> Desktop preview PNG generated ($($fileInfo.Length) bytes): $PreviewPath" -ForegroundColor Green
} else {
    throw "Failed to generate $PreviewPath"
}
