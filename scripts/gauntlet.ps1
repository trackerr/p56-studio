# Gauntlet: run the whole test suite (unit tests + simulator integration tests)
# again and again until it fails or the round count is reached.
#   powershell -File scripts\gauntlet.ps1 -Rounds 20
param([int]$Rounds = 10, [switch]$Release)

$ErrorActionPreference = 'Continue'
Set-Location (Split-Path $PSScriptRoot -Parent)
$profileArg = if ($Release) { '--release' } else { $null }
$log = Join-Path $env:TEMP 'p56-gauntlet.log'
Remove-Item $log -ErrorAction SilentlyContinue

cargo build --workspace --tests $profileArg 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Write-Host 'build failed'; cargo build --workspace --tests $profileArg; exit 1 }

$sw = [Diagnostics.Stopwatch]::StartNew()
for ($i = 1; $i -le $Rounds; $i++) {
    $out = cargo test --workspace $profileArg 2>&1 | ForEach-Object { "$_" }
    $out | Add-Content $log
    $passed = ($out | Select-String -Pattern 'test result: ok\. (\d+) passed' | ForEach-Object { [int]$_.Matches[0].Groups[1].Value } | Measure-Object -Sum).Sum
    if ($LASTEXITCODE -ne 0) {
        Write-Host ("round {0}/{1}: FAILED" -f $i, $Rounds) -ForegroundColor Red
        $out | Select-String -Pattern 'panicked|FAILED|failures:' | Select-Object -First 20 | ForEach-Object { Write-Host $_.Line }
        Write-Host "full log: $log"
        exit 1
    }
    Write-Host ("round {0}/{1}: {2} tests passed ({3:n0} s)" -f $i, $Rounds, $passed, $sw.Elapsed.TotalSeconds) -ForegroundColor Green
}
Write-Host "gauntlet passed: $Rounds rounds without a failure" -ForegroundColor Green
