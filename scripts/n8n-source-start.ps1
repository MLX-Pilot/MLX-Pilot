param(
  [string]$SourcePath = (Join-Path $PSScriptRoot "..\vendor\n8n"),
  [string]$BindHost = "127.0.0.1",
  [int]$Port = 5678
)

$ErrorActionPreference = "Stop"

function Resolve-Tool {
  param([string[]]$Candidates)

  foreach ($candidate in $Candidates) {
    $command = Get-Command $candidate -ErrorAction SilentlyContinue
    if ($command) {
      return $command.Source
    }
  }

  throw "Required tool not found: $($Candidates -join ', ')"
}

function Get-PnpmVersionFromPackageManager {
  param([string]$PackageManager)

  if ($PackageManager -match '^pnpm@(.+)$') {
    return $Matches[1]
  }

  throw "Unsupported packageManager value: $PackageManager"
}

function Resolve-CachedPnpmCli {
  param(
    [string]$CorepackHome,
    [string]$PackageManager
  )

  $version = Get-PnpmVersionFromPackageManager $PackageManager
  $candidate = Join-Path $CorepackHome "v1\pnpm\$version\bin\pnpm.cjs"
  if (Test-Path -LiteralPath $candidate) {
    return $candidate
  }

  return $null
}

function Install-LocalPnpmShim {
  param(
    [string]$RepoRoot,
    [string]$NodeCommand,
    [string]$PnpmCli,
    [string]$CorepackHome
  )

  $shimDir = Join-Path $RepoRoot ".cache\bin"
  New-Item -ItemType Directory -Force -Path $shimDir | Out-Null

  $shimPath = Join-Path $shimDir "pnpm.cmd"
  $shimContent = @(
    "@echo off",
    "set `"COREPACK_HOME=$CorepackHome`"",
    "`"$NodeCommand`" `"$PnpmCli`" %*"
  ) -join "`r`n"
  Set-Content -LiteralPath $shimPath -Value $shimContent -Encoding ASCII

  if (-not (($env:PATH -split ';') -contains $shimDir)) {
    $env:PATH = "$shimDir;$env:PATH"
  }

  return $shimPath
}

$repoRoot = Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")
$corepackHome = Join-Path $repoRoot ".cache\corepack"
$env:COREPACK_HOME = $corepackHome

$resolvedSource = Resolve-Path -LiteralPath $SourcePath -ErrorAction SilentlyContinue
if (-not $resolvedSource) {
  Write-Host "n8n source tree not found at $SourcePath"
  Write-Host "Run first: .\scripts\n8n-source-bootstrap.ps1"
  exit 1
}

$nodeModulesPath = Join-Path $resolvedSource "node_modules"
if (-not (Test-Path -LiteralPath $nodeModulesPath)) {
  Write-Host "n8n dependencies are not installed at $nodeModulesPath"
  Write-Host "Run first: .\scripts\n8n-source-bootstrap.ps1"
  exit 1
}

$packagePath = Join-Path $resolvedSource "package.json"
$package = Get-Content -Raw -LiteralPath $packagePath | ConvertFrom-Json
$nodeCommand = Resolve-Tool @("node.exe", "node.cmd", "node")
$pnpmCli = Resolve-CachedPnpmCli $corepackHome $package.packageManager
if (-not $pnpmCli) {
  Write-Host "$($package.packageManager) is not prepared in $corepackHome"
  Write-Host "Run first: .\scripts\n8n-source-bootstrap.ps1"
  exit 1
}
Install-LocalPnpmShim $repoRoot $nodeCommand $pnpmCli $corepackHome | Out-Null

$baseUrl = "http://${BindHost}:${Port}"
$env:N8N_HOST = $BindHost
$env:N8N_PORT = "$Port"
$env:N8N_PROTOCOL = "http"
$env:N8N_EDITOR_BASE_URL = $baseUrl
$env:WEBHOOK_URL = "$baseUrl/"
$env:N8N_PREVIEW_MODE = "true"

Write-Host "Starting n8n from source: $($resolvedSource.Path)"
Write-Host "Editor: $baseUrl"

Push-Location $resolvedSource
try {
  & $nodeCommand $pnpmCli start
  if ($LASTEXITCODE -ne 0) {
    throw "n8n exited with code $LASTEXITCODE"
  }
}
finally {
  Pop-Location
}
