# Android build / run helper for P56 Studio.
#   powershell -File scripts\android.ps1 debug      # debug-APK (arm64 + x86_64) → dist\
#   powershell -File scripts\android.ps1 build      # release-APK, ondertekend (arm64 + armv7 + x86_64) → dist\
#   powershell -File scripts\android.ps1 emulator   # emulator starten
#   powershell -File scripts\android.ps1 install    # laatste debug-APK op emulator/telefoon zetten
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
$abi = @{ aarch64 = @('aarch64-linux-android', 'arm64-v8a'); armv7 = @('armv7-linux-androideabi', 'armeabi-v7a'); x86_64 = @('x86_64-linux-android', 'x86_64'); i686 = @('i686-linux-android', 'x86') }

function Build-Apk([string[]]$targets, [bool]$release) {
  $mode = if ($release) { 'release' } else { 'debug' }
  Push-Location $app
  try {
    npm --prefix ui run bundle | Out-Null
    $jni = Join-Path $gen 'app\src\main\jniLibs'
    if (Test-Path $jni) { Remove-Item -Recurse -Force $jni }
    foreach ($t in $targets) {
      Write-Host "== Rust voor $t ($mode)"
      $targs = @('tauri', 'android', 'build', '--apk', '--target', $t)
      if (-not $release) { $targs += '--debug' }
      & npx @targs 2>&1 | Where-Object { "$_" -match 'error\[|warning: unused|Finished|symbolic link' } | ForEach-Object { Write-Host "   $_" }
      $so = Join-Path $root "target\$($abi[$t][0])\$mode\libp56_studio_lib.so"
      if (-not (Test-Path $so)) { throw "Rust-bibliotheek voor $t ontbreekt: $so" }
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
    if ($LASTEXITCODE -ne 0) { throw 'gradle mislukt' }
  } finally { Pop-Location }
  New-Item -ItemType Directory -Force $dist | Out-Null
  $apk = Get-ChildItem (Join-Path $gen "app\build\outputs\apk\universal\$mode") -Filter *.apk | Select-Object -First 1
  $name = "P56-Studio-0.2.0-$mode.apk"
  Copy-Item $apk.FullName (Join-Path $dist $name) -Force
  Write-Host "→ dist\$name ($([math]::Round($apk.Length / 1MB, 1)) MB)"
}

switch ($Cmd) {
  'env' { Write-Host "JAVA_HOME=$env:JAVA_HOME`nANDROID_HOME=$env:ANDROID_HOME`nNDK_HOME=$env:NDK_HOME" }
  'debug' { Build-Apk @('aarch64', 'x86_64') $false }
  'build' { Build-Apk @('aarch64', 'armv7', 'x86_64') $true }
  'install' {
    $apk = Join-Path $dist 'P56-Studio-0.2.0-debug.apk'
    adb install -r $apk
  }
  'emulator' {
    if (-not ((& emulator -list-avds) -contains 'p56')) {
      'no' | & "$env:ANDROID_HOME\cmdline-tools\latest\bin\avdmanager.bat" create avd -n p56 -k 'system-images;android-35;google_apis;x86_64' -d pixel_6
    }
    Start-Process emulator -ArgumentList '-avd', 'p56', '-no-snapshot-save', '-no-boot-anim', '-no-audio', '-gpu', 'auto', '-memory', '3072'
  }
  default { Write-Host 'gebruik: android.ps1 debug|build|install|emulator|env' }
}
