[CmdletBinding()]
param(
    [switch]$AllowDirty
)

$ErrorActionPreference = 'Stop'
$root = [System.IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
Push-Location $root
try {
    if (-not $AllowDirty -and (git status --porcelain)) {
        throw 'Release builds require a clean checkout. Commit or intentionally remove changes first.'
    }

    npm ci
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }

    npm test -- --run
    if ($LASTEXITCODE -ne 0) { throw 'frontend tests failed' }

    npm run build
    if ($LASTEXITCODE -ne 0) { throw 'frontend build failed' }

    cargo test --manifest-path src-tauri\Cargo.toml --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed' }

    npm run tauri build
    if ($LASTEXITCODE -ne 0) { throw 'Tauri installer build failed' }

    & (Join-Path $PSScriptRoot 'Collect-ReleaseArtifacts.ps1') -RepositoryRoot $root
} finally {
    Pop-Location
}
