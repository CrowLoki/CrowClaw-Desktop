[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = [System.IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
Push-Location $root
try {
    npm run build
    if ($LASTEXITCODE -ne 0) { throw 'frontend build failed' }

    cargo check --manifest-path src-tauri\Cargo.toml --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust check failed' }

    'CrowClaw worktree validation passed.'
} finally {
    Pop-Location
}
