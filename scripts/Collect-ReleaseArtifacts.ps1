[CmdletBinding()]
param(
    [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot),
    [string]$OutputDirectory = (Join-Path (Split-Path -Parent $PSScriptRoot) 'release')
)

$ErrorActionPreference = 'Stop'
$root = [System.IO.Path]::GetFullPath($RepositoryRoot)
$output = [System.IO.Path]::GetFullPath($OutputDirectory)

if (-not $output.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Release output must remain inside the repository: $output"
}

if (Test-Path -LiteralPath $output) {
    Remove-Item -LiteralPath $output -Recurse -Force
}
New-Item -ItemType Directory -Path $output | Out-Null

$bundleRoot = Join-Path $root 'src-tauri\target\release\bundle'
$assets = @(
    Get-ChildItem -LiteralPath (Join-Path $bundleRoot 'nsis') -Filter '*.exe' -File -ErrorAction SilentlyContinue
    Get-ChildItem -LiteralPath (Join-Path $bundleRoot 'msi') -Filter '*.msi' -File -ErrorAction SilentlyContinue
)

if ($assets.Count -eq 0) {
    throw "No NSIS or MSI installer was found under $bundleRoot"
}

$manifestAssets = foreach ($asset in $assets) {
    $destination = Join-Path $output $asset.Name
    Copy-Item -LiteralPath $asset.FullName -Destination $destination
    $hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
    [pscustomobject]@{
        name = $asset.Name
        bytes = (Get-Item -LiteralPath $destination).Length
        sha256 = $hash
    }
}

$manifestAssets |
    ForEach-Object { "$($_.sha256)  $($_.name)" } |
    Set-Content -LiteralPath (Join-Path $output 'SHA256SUMS.txt') -Encoding utf8NoBOM

$package = Get-Content -Raw -LiteralPath (Join-Path $root 'package.json') | ConvertFrom-Json
$commit = (git -C $root rev-parse HEAD).Trim()
$manifest = [ordered]@{
    schema = 1
    product = 'CrowClaw'
    version = $package.version
    commit = $commit
    generated_at = [DateTimeOffset]::UtcNow.ToString('O')
    platform = 'windows-x64'
    assets = @($manifestAssets)
}
$manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $output 'release-manifest.json') -Encoding utf8NoBOM

$manifestAssets | Format-Table -AutoSize
