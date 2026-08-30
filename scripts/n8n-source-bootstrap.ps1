param(
  [string]$SourcePath = (Join-Path $PSScriptRoot "..\vendor\n8n"),
  [switch]$SkipBuild,
  [switch]$ProductionBundle,
  [int]$Concurrency = 1
)

$ErrorActionPreference = "Stop"

function Invoke-Step {
  param(
    [string]$Name,
    [scriptblock]$Command
  )

  Write-Host ""
  Write-Host "==> $Name"
  & $Command
  if ($LASTEXITCODE -ne 0) {
    throw "$Name failed with exit code $LASTEXITCODE"
  }
}

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
New-Item -ItemType Directory -Force -Path $corepackHome | Out-Null
$env:COREPACK_HOME = $corepackHome

$sourcePackagePath = Join-Path $SourcePath "package.json"
if (Test-Path -LiteralPath $sourcePackagePath) {
  Write-Host "n8n source already present: $SourcePath"
} else {
  Push-Location $repoRoot
  try {
    Invoke-Step "Initialize n8n submodule" {
      & git submodule update --init --recursive vendor/n8n
    }
  }
  finally {
    Pop-Location
  }
}

$resolvedSource = Resolve-Path -LiteralPath $SourcePath
$packagePath = Join-Path $resolvedSource "package.json"
$package = Get-Content -Raw -LiteralPath $packagePath | ConvertFrom-Json
$corepackCommand = Resolve-Tool @("corepack.cmd", "corepack.exe", "corepack")
$nodeCommand = Resolve-Tool @("node.exe", "node.cmd", "node")
$pnpmCli = Resolve-CachedPnpmCli $corepackHome $package.packageManager

Push-Location $resolvedSource
try {
  if (-not $pnpmCli) {
    Invoke-Step "Prepare $($package.packageManager)" {
      & $corepackCommand prepare $package.packageManager --activate
    }
    $pnpmCli = Resolve-CachedPnpmCli $corepackHome $package.packageManager
  }

  if (-not $pnpmCli) {
    throw "$($package.packageManager) was not prepared in $corepackHome"
  }

  $pnpmShim = Install-LocalPnpmShim $repoRoot $nodeCommand $pnpmCli $corepackHome
  Write-Host "Using local pnpm shim: $pnpmShim"

  function Invoke-Pnpm {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    & $nodeCommand $pnpmCli @Arguments
  }

  Invoke-Step "Install n8n dependencies" {
    Invoke-Pnpm install --frozen-lockfile
  }

  if (-not $SkipBuild) {
    if ($ProductionBundle) {
      Invoke-Step "Build n8n production bundle" {
        Invoke-Pnpm "build:n8n"
      }
    } else {
      Invoke-Step "Build n8n local runtime" {
        Invoke-Pnpm turbo run build "--filter=n8n..." "--output-logs=full" "--concurrency=$Concurrency"
      }
    }
  }
}
finally {
  Pop-Location
}

Write-Host ""
Write-Host "n8n source bootstrap complete."
Write-Host "Start it with: .\scripts\n8n-source-start.ps1"
