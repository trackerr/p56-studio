# Android build / run helper for P56 Studio.
#   powershell -File scripts\android.ps1 build      # APK's (release, ondertekend) → dist\
#   powershell -File scripts\android.ps1 debug      # debug-APK (x86_64 + arm64)
#   powershell -File scripts\android.ps1 emulator   # emulator starten
# Toolchain: C:\Users\track\.p56-android (JDK 17, SDK, NDK) — zie README.
param([Parameter(Position = 0)][string]$Cmd = 'build')

$ErrorActionPreference = 'Stop'
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

switch ($Cmd) {
  'env' { Write-Host "JAVA_HOME=$env:JAVA_HOME`nANDROID_HOME=$env:ANDROID_HOME`nNDK_HOME=$env:NDK_HOME" }
  'debug' {
    Push-Location $app
    npx tauri android build --debug --apk --target aarch64 x86_64
    Pop-Location
  }
  'build' {
    Push-Location $app
    npx tauri android build --apk --target aarch64 armv7 x86_64
    Pop-Location
    $dist = Join-Path $root 'dist'
    New-Item -ItemType Directory -Force $dist | Out-Null
    Get-ChildItem (Join-Path $app 'src-tauri\gen\android\app\build\outputs\apk') -Recurse -Filter *.apk |
      Where-Object { $_.FullName -match 'release' } | ForEach-Object { Copy-Item $_.FullName $dist -Force; Write-Host "→ dist\$($_.Name)" }
  }
  'emulator' {
    $avd = 'p56'
    if (-not ((& emulator -list-avds) -contains $avd)) {
      'no' | & "$env:ANDROID_HOME\cmdline-tools\latest\bin\avdmanager.bat" create avd -n $avd -k 'system-images;android-35;google_apis;x86_64' -d pixel_6
    }
    Start-Process emulator -ArgumentList '-avd', $avd, '-no-snapshot-save', '-no-boot-anim', '-gpu', 'auto'
  }
  default { Write-Host 'gebruik: android.ps1 build|debug|emulator|env' }
}
