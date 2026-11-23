param(
    [switch]$TestOnly,
    [switch]$Window,
    [switch]$NoInstall,
    [string]$AdbHost = "127.0.0.1:5555",
    [string]$SavePng = ""
)

$ErrorActionPreference = "Stop"
$WorkspaceRoot = Split-Path -Parent $PSScriptRoot
Set-Location $WorkspaceRoot

if ($SavePng -ne "") {
    Write-Host "[Android App] Exporting 390x844 phone screenshot to $SavePng..." -ForegroundColor Cyan
    cargo run -p android_app --bin android_app_cli --features winit -- --save-png $SavePng
    if ($LASTEXITCODE -ne 0) { throw "cargo run --save-png failed" }
    exit 0
}

if ($Window) {
    Write-Host "[Android App] Launching interactive 390x844 Android Phone Simulator Window..." -ForegroundColor Green
    cargo run -p android_app --bin android_app_cli --features winit -- --window
    if ($LASTEXITCODE -ne 0) { throw "cargo run --window failed" }
    exit 0
}

Write-Host "[1/4] Running Android App integration tests (SafeArea, Touch, Swipe-Back, IME, Back Button, Rotation)..." -ForegroundColor Cyan
cargo test -p android_app
if ($LASTEXITCODE -ne 0) { throw "cargo test -p android_app failed" }

if ($TestOnly) {
    exit 0
}

$SdkRoot = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { "C:\Users\wang-\AppData\Local\Android\Sdk" }
$NdkRoot = Join-Path $SdkRoot "ndk\26.3.11579264\toolchains\llvm\prebuilt\windows-x86_64"
$NdkBin = Join-Path $NdkRoot "bin"
$UnwindA = Join-Path $NdkRoot "lib\clang\17\lib\linux\aarch64\libunwind.a"
$BuildTools = Join-Path $SdkRoot "build-tools\30.0.3"
$AndroidJar = Join-Path $SdkRoot "platforms\android-29\android.jar"
$AdbExe = Join-Path $SdkRoot "platform-tools\adb.exe"
$JavaHome = if ($env:JAVA_HOME) { $env:JAVA_HOME } else { "C:\Users\wang-\.dev\jdk-21.0.7" }
$JavaExe = Join-Path $JavaHome "bin\java.exe"
$JavacExe = Join-Path $JavaHome "bin\javac.exe"

Write-Host "[2/4] Cross-compiling native JNI library libandroid_app.so (arm64-v8a)..." -ForegroundColor Cyan
$StubDir = Join-Path $WorkspaceRoot "target\android_stubs"
New-Item -ItemType Directory -Force -Path $StubDir | Out-Null

$CompatC = @"
#include <string.h>
#include <errno.h>

int bcmp(const void *s1, const void *s2, size_t n) {
    return memcmp(s1, s2, n);
}

int *__errno_location(void) {
    return __errno();
}

int __xpg_strerror_r(int errnum, char *buf, size_t buflen) {
    return strerror_r(errnum, buf, buflen);
}

int statx(int dirfd, const char *pathname, int flags, unsigned int mask, void *statxbuf) {
    (void)dirfd; (void)pathname; (void)flags; (void)mask; (void)statxbuf;
    errno = ENOSYS;
    return -1;
}
"@
Set-Content -Path "$StubDir\bionic_compat.c" -Value $CompatC -Encoding UTF8
& "$NdkBin\clang.exe" --target=aarch64-linux-android29 -O2 -fPIC -c "$StubDir\bionic_compat.c" -o "$StubDir\bionic_compat.o"
Copy-Item $UnwindA "$StubDir\libgcc_s.a" -Force
& "$NdkBin\llvm-ar.exe" rcs "$StubDir\libgcc_s.a" "$StubDir\bionic_compat.o"
& "$NdkBin\llvm-ar.exe" rc "$StubDir\libutil.a"
& "$NdkBin\llvm-ar.exe" rc "$StubDir\librt.a"
& "$NdkBin\llvm-ar.exe" rc "$StubDir\libpthread.a"

$env:CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER = "$NdkBin\clang.exe"
$env:CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS = "-C link-arg=--target=aarch64-linux-android29 -C link-arg=-L$StubDir -C link-arg=-landroid -C link-arg=-llog"
cargo build --release -p android_app --lib --target aarch64-unknown-linux-gnu
if ($LASTEXITCODE -ne 0) { throw "Failed to build libandroid_app.so" }

Write-Host "[3/4] Emitting Android host glue from velox-core::platform::android and compiling MainActivity.java -> classes.dex..." -ForegroundColor Cyan
$ApkWorkDir = Join-Path $WorkspaceRoot "target\android_apk_build"
if (Test-Path $ApkWorkDir) { Remove-Item -Recurse -Force $ApkWorkDir }
$HostGenDir = Join-Path $ApkWorkDir "host"
$ClassesDir = Join-Path $ApkWorkDir "classes"
$DexDir = Join-Path $ApkWorkDir "dex"
New-Item -ItemType Directory -Force -Path $HostGenDir, $ClassesDir, $DexDir | Out-Null

cargo run -p android_app --bin android_app_cli -- --emit-host $HostGenDir
if ($LASTEXITCODE -ne 0) { throw "Failed to emit Android host files" }

$JavaSrc = Join-Path $HostGenDir "src\com\velox\android\MainActivity.java"
& $JavacExe -Xlint:-options -Xlint:-deprecation --release 8 -cp $AndroidJar -d $ClassesDir $JavaSrc
if ($LASTEXITCODE -ne 0) { throw "javac compilation failed" }

$InnerClasses = Get-ChildItem -Path (Join-Path $ClassesDir "com\velox\android") -Filter "*.class" | ForEach-Object { $_.FullName }
& $JavaExe -cp "$BuildTools\lib\d8.jar" com.android.tools.r8.D8 --no-desugaring --output $DexDir $InnerClasses
if ($LASTEXITCODE -ne 0) { throw "d8 dexing failed" }

$ManifestPath = Join-Path $HostGenDir "AndroidManifest.xml"
$UnalignedApk = Join-Path $ApkWorkDir "unaligned.apk"
& "$BuildTools\aapt2.exe" link -o $UnalignedApk -I $AndroidJar --manifest $ManifestPath
if ($LASTEXITCODE -ne 0) { throw "aapt2 link failed" }

$SoPath = Join-Path $WorkspaceRoot "target\aarch64-unknown-linux-gnu\release\libandroid_app.so"
$DexPath = Join-Path $DexDir "classes.dex"

python -c "
import zipfile
apk_path = r'$UnalignedApk'
dex_path = r'$DexPath'
so_path  = r'$SoPath'
with zipfile.ZipFile(apk_path, 'a') as zf:
    zf.write(dex_path, 'classes.dex', compress_type=zipfile.ZIP_DEFLATED)
    zf.write(so_path, 'lib/arm64-v8a/libandroid_app.so', compress_type=zipfile.ZIP_STORED)
"
if ($LASTEXITCODE -ne 0) { throw "Failed to inject classes.dex and libandroid_app.so into APK" }

Write-Host "[4/4] Aligning and signing APK (zipalign + apksigner)..." -ForegroundColor Cyan
$AlignedApk = Join-Path $ApkWorkDir "aligned.apk"
& "$BuildTools\zipalign.exe" -f -p 4 $UnalignedApk $AlignedApk
if ($LASTEXITCODE -ne 0) { throw "zipalign failed" }

$Keystore = "C:\Users\wang-\.android\debug.keystore"
$FinalApk = Join-Path $WorkspaceRoot "target\android_app-debug.apk"
$ExampleApk = Join-Path $WorkspaceRoot "examples\android_app\android_app-debug.apk"
& $JavaExe -jar "$BuildTools\lib\apksigner.jar" sign --ks $Keystore --ks-pass pass:android --key-pass pass:android --out $FinalApk $AlignedApk
if ($LASTEXITCODE -ne 0) { throw "apksigner failed" }
Copy-Item $FinalApk $ExampleApk -Force

$ApkSizeKb = [math]::Round((Get-Item $FinalApk).Length / 1024, 1)
Write-Host "      -> Successfully built signed Android APK ($ApkSizeKb KB):" -ForegroundColor Green
Write-Host "         1. $FinalApk" -ForegroundColor Green
Write-Host "         2. $ExampleApk" -ForegroundColor Green

if (-not $NoInstall -and (Test-Path $AdbExe)) {
    Write-Host "[ADB] Connecting to $AdbHost and installing APK..." -ForegroundColor Cyan
    & $AdbExe connect $AdbHost | Out-Host
    & $AdbExe -s $AdbHost install -r $FinalApk | Out-Host
    if ($LASTEXITCODE -eq 0) {
        Write-Host "[ADB] Launching com.velox.android/.MainActivity on $AdbHost..." -ForegroundColor Green
        & $AdbExe -s $AdbHost shell am start -n com.velox.android/.MainActivity | Out-Host
    }
}
