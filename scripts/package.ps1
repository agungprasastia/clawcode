[CmdletBinding()]
param(
    [string]$TargetDir = "target\release-package"
)

$ErrorActionPreference = "Stop"

cargo build --release --locked
New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null
$binary = Join-Path $TargetDir "clawcode.exe"
Copy-Item "target\release\clawcode.exe" $binary -Force
$hash = (Get-FileHash $binary -Algorithm SHA256).Hash.ToLowerInvariant()
Set-Content -Path "$binary.sha256" -Value "$hash  clawcode.exe"
& $binary --version *> $null
if ($LASTEXITCODE -ne 0) { throw "packaged binary validation failed with exit code $LASTEXITCODE" }
Write-Output "Packaged $binary"
