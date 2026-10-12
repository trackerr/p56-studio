# Android build / run helper for P56 Studio.
#   powershell -File scripts\android.ps1 debug      # debug APK (arm64 + x86_64) → dist\
#   powershell -File scripts\android.ps1 build      # signed release APK (arm64: every current phone) → dist\
#   powershell -File scripts\android.ps1 phone      # quick signed APK for a phone (arm64, no LTO), installed and started
#   powershell -File scripts\android.ps1 emulator   # start the emulator
#   powershell -File scripts\android.ps1 install    # install the latest debug APK on the emulator/phone
# Toolchain in %USERPROFILE%\.p56-android (JDK 17, SDK, NDK).
#
# Tauri links the Rust library into jniLibs with a symlink, which Windows only allows in
# Developer Mode. This script copies the library instead and runs Gradle itself.
param([Parameter(Position = 0)][string]$Cmd = 'build')

$ErrorActionPreference = 'Continue'
$base = Join-Path $env:USERPROFILE '.p56-android'
$env:JAVA_HOME = Join-Path $base 'jdk17'
$env:ANDROID_HOME = Join-Path $base 'sdk'
$env:ANDROID_SDK_ROOT = $env:ANDROID_HOME
$env:ANDROID_AVD_HOME = Join-Path $base 'avd'
$ndk = Get-ChildItem (Join-Path $env:ANDROID_HOME 'ndk') -Directory | Sort-Object Name -Descending | Select-Object -First 1
$env:NDK_HOME = $ndk.FullName
$env:PATH = "$env:JAVA_HOME\bin;$env:ANDROID_HOME\platform-tools;$env:ANDROID_HOME\emulator;$env:PATH"
$root = Split-Path $PSScriptRoot -Parent
$app = Join-Path $root 'app'
$gen = Join-Path $app 'src-tauri\gen\android'
$dist = Join-Path $root 'dist'
$version = (Get-Content (Join-Path $app 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
$abi = @{ aarch64 = @('aarch64-linux-android', 'arm64-v8a'); armv7 = @('armv7-linux-androideabi', 'armeabi-v7a'); x86_64 = @('x86_64-linux-android', 'x86_64'); i686 = @('i686-linux-android', 'x86') }

function Build-Apk([string[]]$targets, [bool]$release) {
  $mode = if ($release) { 'release' } else { 'debug' }
  Push-Location $app
  try {
    npm --prefix ui run bundle | Out-Null
    $jni = Join-Path $gen 'app\src\main\jniLibs'
    if (Test-Path $jni) { Remove-Item -Recurse -Force $jni }
    foreach ($t in $targets) {
      Write-Host "== Rust for $t ($mode)"
      $targs = @('tauri', 'android', 'build', '--apk', '--target', $t)
      if (-not $release) { $targs += '--debug' }
      & npx @targs 2>&1 | Where-Object { "$_" -match 'error\[|warning: unused|Finished|symbolic link' } | ForEach-Object { Write-Host "   $_" }
      $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
      $so = Join-Path $targetDir "$($abi[$t][0])\$mode\libp56_studio_lib.so"
      if (-not (Test-Path $so)) { throw "Rust library for $t is missing: $so" }
      $dst = Join-Path $jni $abi[$t][1]
      New-Item -ItemType Directory -Force $dst | Out-Null
      Copy-Item $so $dst -Force
    }
  } finally { Pop-Location }
  Write-Host "== Gradle ($mode)"
  Push-Location $gen
  try {
    $task = if ($release) { 'Release' } else { 'Debug' }
    .\gradlew.bat "assembleUniversal$task" -x "rustBuildUniversal$task" --console=plain -q
    if ($LASTEXITCODE -ne 0) { throw 'gradle failed' }
  } finally { Pop-Location }
  New-Item -ItemType Directory -Force $dist | Out-Null
  $apk = Get-ChildItem (Join-Path $gen "app\build\outputs\apk\universal\$mode") -Filter *.apk | Select-Object -First 1
  $name = "P56-Studio-$version-$mode.apk"
  Copy-Item $apk.FullName (Join-Path $dist $name) -Force
  Write-Host "→ dist\$name ($([math]::Round($apk.Length / 1MB, 1)) MB)"
}

switch ($Cmd) {
  'env' { Write-Host "JAVA_HOME=$env:JAVA_HOME`nANDROID_HOME=$env:ANDROID_HOME`nNDK_HOME=$env:NDK_HOME" }
  'debug' { Build-Apk @('aarch64', 'x86_64') $false }
  'build' { Build-Apk @('aarch64') $true }
  'phone' {
    # A third of the Rust work (arm64 only) and much faster linking (no LTO, parallel codegen),
    # in its own target folder so the release build's cache stays intact. For testing; releases
    # use 'build'.
    $env:CARGO_TARGET_DIR = Join-Path $root 'target\phone'
    $env:CARGO_PROFILE_RELEASE_LTO = 'false'
    $env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = '16'
    Build-Apk @('aarch64') $true
    # the first connected device (set ANDROID_SERIAL to choose another)
    adb install -r (Join-Path $dist "P56-Studio-$version-release.apk")
    if ($LASTEXITCODE -eq 0) { adb shell monkey -p nl.trackerr.p56studio -c android.intent.category.LAUNCHER 1 | Out-Null }
  }
  'install' {
    $apk = Join-Path $dist "P56-Studio-$version-debug.apk"
    adb install -r $apk
  }
  'emulator' {
    if (-not ((& emulator -list-avds) -contains 'p56')) {
      'no' | & "$env:ANDROID_HOME\cmdline-tools\latest\bin\avdmanager.bat" create avd -n p56 -k 'system-images;android-35;google_apis;x86_64' -d pixel_6
    }
    Start-Process emulator -ArgumentList '-avd', 'p56', '-no-snapshot-save', '-no-boot-anim', '-no-audio', '-gpu', 'auto', '-memory', '3072'
  }
  default { Write-Host 'usage: android.ps1 debug|build|phone|install|emulator|env' }
}
