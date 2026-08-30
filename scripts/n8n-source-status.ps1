param(
  [string]$SourcePath = (Join-Path $PSScriptRoot "..\vendor\n8n")
)

$ErrorActionPreference = "Stop"

function Write-Value {
  param(
    [string]$Label,
    [string]$Value
  )

  if ([string]::IsNullOrWhiteSpace($Value)) {
    $Value = "-"
  }

  Write-Host ("{0,-18} {1}" -f ($Label + ":"), $Value)
}

function Resolve-Tool {
  param([string[]]$Candidates)

  foreach ($candidate in $Candidates) {
    $command = Get-Command $candidate -ErrorAction SilentlyContinue
    if ($command) {
      return $command.Source
    }
  }

  return $null
}

function Get-PnpmVersionFromPackageManager {
  param([string]$PackageManager)

  if ($PackageManager -match '^pnpm@(.+)$') {
    return $Matches[1]
  }

  return $null
}

$repoRoot = Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")
$resolvedSource = Resolve-Path -LiteralPath $SourcePath -ErrorAction SilentlyContinue
if (-not $resolvedSource) {
  Write-Host "n8n source tree not found at $SourcePath"
  Write-Host "Run from the repo root: git submodule update --init --recursive vendor/n8n"
  exit 1
}

$packagePath = Join-Path $resolvedSource "package.json"
if (-not (Test-Path -LiteralPath $packagePath)) {
  Write-Host "package.json not found at $packagePath"
  exit 1
}

$package = Get-Content -Raw -LiteralPath $packagePath | ConvertFrom-Json
$nodeCommand = Resolve-Tool @("node.exe", "node.cmd", "node")
$pnpmCommand = Resolve-Tool @("pnpm.cmd", "pnpm.exe", "pnpm")
$corepackCommand = Resolve-Tool @("corepack.cmd", "corepack.exe", "corepack")
$corepackHome = Join-Path $repoRoot ".cache\corepack"
$pnpmVersion = Get-PnpmVersionFromPackageManager $package.packageManager
$cachedPnpmCli = if ($pnpmVersion) {
  Join-Path $corepackHome "v1\pnpm\$pnpmVersion\bin\pnpm.cjs"
} else {
  $null
}
$localPnpmShim = Join-Path $repoRoot ".cache\bin\pnpm.cmd"

Write-Host "n8n source status"
Write-Host ""
Write-Value "source" $resolvedSource.Path
Write-Value "n8n version" $package.version
Write-Value "package manager" $package.packageManager
Write-Value "required node" $package.engines.node
Write-Value "required pnpm" $package.engines.pnpm
Write-Value "local node" $(if ($nodeCommand) { (& $nodeCommand --version) } else { "not found" })
Write-Value "pnpm command" $(if ($pnpmCommand) { $pnpmCommand } else { "not found" })
Write-Value "corepack" $(if ($corepackCommand) { (& $corepackCommand --version) } else { "not found" })
Write-Value "corepack home" $corepackHome
Write-Value "cached pnpm" $(if ($cachedPnpmCli -and (Test-Path -LiteralPath $cachedPnpmCli)) { $cachedPnpmCli } else { "not prepared" })
Write-Value "local pnpm shim" $(if (Test-Path -LiteralPath $localPnpmShim) { $localPnpmShim } else { "not created yet" })

if (-not $pnpmCommand) {
  Write-Host ""
  Write-Host "pnpm not found in PATH. The bootstrap script will use corepack.cmd directly with COREPACK_HOME inside this repo."
} else {
  Write-Host ""
  Write-Host "pnpm was found, but this status check does not execute it. On Windows it may be a Corepack shim."
}
