<#
.SYNOPSIS
  Fetches a prebuilt llama.cpp engine and stages it for bundling with MLX Pilot.

.DESCRIPTION
  Downloads an official llama.cpp Windows release and extracts the `llama-server`
  launcher plus its shared libraries into the Tauri `binaries/llamacpp` resource
  folder. This is the CPU "baseline" engine that ships inside the installer so that
  local inference works offline on first run. Accelerated builds (Vulkan/CUDA) are
  fetched automatically at runtime by the daemon based on detected hardware, so they
  are NOT bundled here.

  Binaries are intentionally git-ignored — run this script once before building the
  installer (or let it run as part of your release pipeline).

.PARAMETER Release
  Upstream release tag to pull (must match DEFAULT_RELEASE in the daemon provisioner).

.PARAMETER Variant
  Engine variant to bundle. Defaults to `cpu` (universal, no GPU/driver requirements).

.EXAMPLE
  ./scripts/fetch-llama-engine.ps1
  ./scripts/fetch-llama-engine.ps1 -Release b9601 -Variant cpu
#>
[CmdletBinding()]
param(
  [string]$Release = 'b9601',
  [ValidateSet('cpu', 'vulkan', 'cuda')]
  [string]$Variant = 'cpu',
  [switch]$Force
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$destDir = Join-Path $repoRoot 'apps\desktop-ui\src-tauri\binaries\llamacpp'
$serverExe = Join-Path $destDir 'llama-server.exe'

if ((Test-Path $serverExe) -and -not $Force) {
  Write-Host "[fetch-llama-engine] Engine ja presente em $destDir (use -Force para refazer)."
  exit 0
}

switch ($Variant) {
  'cpu'    { $asset = "llama-$Release-bin-win-cpu-x64.zip" }
  'vulkan' { $asset = "llama-$Release-bin-win-vulkan-x64.zip" }
  'cuda'   { $asset = "llama-$Release-bin-win-cuda-12.4-x64.zip" }
}

$url = "https://github.com/ggml-org/llama.cpp/releases/download/$Release/$asset"
$tmpZip = Join-Path ([System.IO.Path]::GetTempPath()) "llama-$Release-$Variant.zip"
$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) "llama-$Release-$Variant"

Write-Host "[fetch-llama-engine] Baixando $asset ..."
Invoke-WebRequest -Uri $url -OutFile $tmpZip -UseBasicParsing -TimeoutSec 600

if (Test-Path $tmpDir) { Remove-Item -Recurse -Force $tmpDir }
Expand-Archive -Path $tmpZip -DestinationPath $tmpDir -Force

New-Item -ItemType Directory -Force -Path $destDir | Out-Null
# Keep every shared library plus the server launcher; the other CLI exes are unused.
$files = Get-ChildItem -Path $tmpDir -File | Where-Object {
  $_.Extension -eq '.dll' -or $_.Name -eq 'llama-server.exe'
}
foreach ($f in $files) {
  Copy-Item $f.FullName -Destination $destDir -Force
}

Remove-Item -Force $tmpZip -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue

$count = (Get-ChildItem -Path $destDir -File | Measure-Object).Count
Write-Host "[fetch-llama-engine] OK: $count arquivos em $destDir (variant=$Variant, release=$Release)."
