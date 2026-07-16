[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = [System.IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))

if (-not (Test-Path -LiteralPath (Join-Path $root '.git'))) {
    throw "CrowClaw Git checkout not found at $root"
}
if (-not (Test-Path -LiteralPath (Join-Path $root 'package-lock.json'))) {
    throw "CrowClaw package lock is missing at $root"
}
if (-not (Test-Path -LiteralPath (Join-Path $root 'src-tauri\Cargo.toml'))) {
    throw "CrowClaw Rust manifest is missing at $root"
}

Push-Location $root
try {
    npm ci --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }

    cargo metadata --manifest-path src-tauri\Cargo.toml --no-deps --format-version 1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }

    [pscustomobject]@{
        repository = $root
        branch = (git branch --show-current).Trim()
        commit = (git rev-parse --short HEAD).Trim()
        node = (node --version).Trim()
        rust = (rustc --version).Trim()
        ready = $true
    } | Format-List
} finally {
    Pop-Location
}
