# Safe post-package verification for the affected Windows VFS runtime.
# Does not pair apps, write user files, change installed binaries or publish anything.
param(
  [ValidateSet(47833,47834,47835,47836)] [int]$Port = 47833,
  [switch]$IncludeSlowTimeout,
  [switch]$SkipCargo,
  [string]$OutputDirectory = 'VFS_0.2.2_VALIDATION'
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$target = Join-Path $root $OutputDirectory
New-Item -Path $target -ItemType Directory -Force | Out-Null
$report = Join-Path $target 'validation-results.txt'
"VontaqFS 0.2.2 | Windows candidate diagnostic | $(Get-Date -Format o)" | Set-Content $report -Encoding utf8
$issues = 0
function Invoke-Stage {
  param([string]$Name, [scriptblock]$Script)
  Write-Host "==== $Name ===="
  Add-Content $report "==== $Name ===="
  try {
    $global:LASTEXITCODE = 0
    $out = & $Script 2>&1 | Out-String
    $out | Add-Content $report
    Write-Host $out
    if ($LASTEXITCODE -ne 0) { throw "exit code $LASTEXITCODE" }
    Add-Content $report "STATUS: PASS"
  } catch {
    $script:issues += 1
    $message = $_.Exception.Message
    Add-Content $report "STATUS: FAILED ($message)"
    Write-Warning "$Name failed: $message"
  }
}
Push-Location $root
try {
  Invoke-Stage 'Installed runtime identity (PID, executable, SHA, signer)' {
    & powershell -NoProfile -File scripts\diagnostics\vfs-installed-provenance.ps1 -Port $Port
  }
  Invoke-Stage 'Real installed runtime TCP ingress, no real credentials or storage changes' {
    $extra = @()
    if ($IncludeSlowTimeout) { $extra += '--include-slow' }
    & node scripts\diagnostics\vfs-tcp-proof.mjs --port "$Port" @extra
  }
  Invoke-Stage 'Release metadata and SDK independence' {
    & node scripts\release\verify-release.mjs --tag v0.2.2
  }
  Invoke-Stage 'Release and compatibility contract tests' {
    & npm run test:integration
  }
  if (-not $SkipCargo) {
    Invoke-Stage 'Windows source runtime tests, release profile, actual loopback TCP' {
      # Cargo progress is emitted to stderr. Merge it inside cmd.exe so Windows PowerShell 5.1
      # does not promote normal "Compiling ..." messages to terminating NativeCommandError.
      # cmd.exe preserves the actual cargo exit code for Invoke-Stage.
      & cmd.exe /D /C "cargo test -p vontaqfs-runtime --release tcp_ingress_ -- --test-threads=1 2>&1"
    }
  }
} finally {
  Pop-Location
}
Add-Content $report "FAILED_STAGES: $issues"
Write-Host "Validation report: $report"
if ($issues -gt 0) { exit 1 }
exit 0
