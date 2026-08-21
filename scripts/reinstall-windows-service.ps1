#Requires -RunAsAdministrator
<#
.SYNOPSIS
  Rebuild netvan-api release binary, remove the old Windows service, install and start the new one.

.DESCRIPTION
  Run this after any code change that affects the installed netvan-api Windows service.
  Must be elevated (Administrator).
#>
$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

$Exe = Join-Path $RepoRoot "target\release\netvan-api.exe"

Write-Host "==> cargo build -p netvan-api --release"
cargo build -p netvan-api --release
if ($LASTEXITCODE -ne 0) {
  throw "cargo build failed with exit code $LASTEXITCODE"
}

if (-not (Test-Path $Exe)) {
  throw "Missing binary: $Exe"
}

Write-Host "==> stop (ignore if not running)"
& $Exe stop 2>$null

Write-Host "==> uninstall old service"
& $Exe uninstall
if ($LASTEXITCODE -ne 0) {
  # Not installed yet is OK on first install
  Write-Host "uninstall exited $LASTEXITCODE (continuing if service was absent)"
}

Write-Host "==> install new service from $Exe"
& $Exe install
if ($LASTEXITCODE -ne 0) {
  throw "install failed with exit code $LASTEXITCODE"
}

Write-Host "==> start"
& $Exe start
if ($LASTEXITCODE -ne 0) {
  throw "start failed with exit code $LASTEXITCODE"
}

Write-Host "==> status"
& $Exe status

Write-Host "Done: old service removed, new release installed and started."
