param(
    [switch]$TestOnly,
    [switch]$NoServe,
    [switch]$Window,
    [int]$Port = 8080,
    [string]$SavePng = ""
)

$ErrorActionPreference = "Stop"
$WorkspaceRoot = Split-Path -Parent $PSScriptRoot
Set-Location $WorkspaceRoot

if ($SavePng -ne "") {
    Write-Host "[Web App] Exporting 800x600 Web Canvas screenshot to $SavePng..." -ForegroundColor Cyan
    cargo run -p web_app --bin web_app_cli --features winit -- --save-png $SavePng
    if ($LASTEXITCODE -ne 0) { throw "cargo run --save-png failed" }
    exit 0
}

if ($Window) {
    Write-Host "[Web App] Launching interactive 800x600 Web Canvas Preview Window..." -ForegroundColor Green
    cargo run -p web_app --bin web_app_cli --features winit -- --window
    if ($LASTEXITCODE -ne 0) { throw "cargo run --window failed" }
    exit 0
}

Write-Host "[1/3] Running WebAssembly Canvas integration tests..." -ForegroundColor Cyan
cargo test -p web_app
if ($LASTEXITCODE -ne 0) { throw "cargo test -p web_app failed" }

if ($TestOnly) {
    exit 0
}

Write-Host "[2/3] Building Core WebAssembly module web_app.wasm..." -ForegroundColor Cyan
$WasmSrc = Join-Path $WorkspaceRoot "target\wasm32-wasip2\release\web_app.wasm"
if (Test-Path $WasmSrc) {
    $Hdr = [System.IO.File]::ReadAllBytes($WasmSrc)[0..7]
    if ($Hdr[4] -ne 1) {
        Remove-Item $WasmSrc -Force
    }
}

$PrevLinker = $env:CARGO_TARGET_WASM32_WASIP2_LINKER
$env:CARGO_TARGET_WASM32_WASIP2_LINKER = "rust-lld"
cargo build --release -p web_app --lib --target wasm32-wasip2
$BuildCode = $LASTEXITCODE
$env:CARGO_TARGET_WASM32_WASIP2_LINKER = $PrevLinker
if ($BuildCode -ne 0) { throw "WASM build failed" }

$DistDir = Join-Path $WorkspaceRoot "target\web_dist"
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

$DistWasm = Join-Path $DistDir "web_app.wasm"
$ExampleWasm = Join-Path $WorkspaceRoot "examples\web_app\web_app.wasm"
Copy-Item $WasmSrc $DistWasm -Force
Copy-Item $WasmSrc $ExampleWasm -Force

$HtmlSrc = Join-Path $WorkspaceRoot "examples\web_app\index.html"
$DistHtml = Join-Path $DistDir "index.html"
Copy-Item $HtmlSrc $DistHtml -Force

$WasmBytes = [System.IO.File]::ReadAllBytes($DistWasm)[0..7]
$WasmHeaderHex = ($WasmBytes | ForEach-Object { $_.ToString("x2") }) -join " "
$WasmSizeKb = [math]::Round((Get-Item $DistWasm).Length / 1024, 1)
Write-Host "      -> Built Core WebAssembly v1 ($WasmSizeKb KB, header: $WasmHeaderHex):" -ForegroundColor Green
Write-Host "         - $DistHtml" -ForegroundColor Green
Write-Host "         - $DistWasm" -ForegroundColor Green
Write-Host "         - $ExampleWasm" -ForegroundColor Green

if (-not $NoServe) {
    Write-Host "[3/3] Serving Web App at http://127.0.0.1:$Port/ (No-Cache HTTP Server, Press Ctrl+C to stop)..." -ForegroundColor Green
    $PyServer = @"
import http.server, socketserver, sys
 directory = sys.argv[1]
port = int(sys.argv[2])
class NoCacheHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=directory, **kwargs)
    def end_headers(self):
        self.send_header('Cache-Control', 'no-store, no-cache, must-revalidate, max-age=0')
        self.send_header('Pragma', 'no-cache')
        self.send_header('Expires', '0')
        super().end_headers()
NoCacheHandler.extensions_map['.wasm'] = 'application/wasm'
socketserver.TCPServer.allow_reuse_address = True
with socketserver.TCPServer(('127.0.0.1', port), NoCacheHandler) as httpd:
    httpd.serve_forever()
"@.Replace("`r`n", "`n").Replace(" directory =", "directory =")
    python -c $PyServer $DistDir $Port
} else {
    Write-Host "[3/3] Web bundle ready at $DistDir (pass without -NoServe to start HTTP server)." -ForegroundColor Green
}
