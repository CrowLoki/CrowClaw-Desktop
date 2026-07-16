[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$SourcePath,

    [Parameter(Mandatory)]
    [string]$ManifestPath,

    [Parameter(Mandatory)]
    [string]$OutputPath,

    [Parameter(Mandatory)]
    [string]$RunUrl,

    [Parameter(Mandatory)]
    [string]$ExpectedVersion,

    [Parameter(Mandatory)]
    [string]$ExpectedCommit
)

$ErrorActionPreference = 'Stop'
$source = [System.IO.Path]::GetFullPath($SourcePath)
$manifestFile = [System.IO.Path]::GetFullPath($ManifestPath)
$output = [System.IO.Path]::GetFullPath($OutputPath)

if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
    throw "Release notes source does not exist: $source"
}
if (-not (Test-Path -LiteralPath $manifestFile -PathType Leaf)) {
    throw "Release manifest does not exist: $manifestFile"
}
if (-not [Uri]::IsWellFormedUriString($RunUrl, [UriKind]::Absolute)) {
    throw "GitHub Actions run URL is not absolute: $RunUrl"
}

$manifest = Get-Content -Raw -LiteralPath $manifestFile | ConvertFrom-Json
if ([string]$manifest.version -ne $ExpectedVersion) {
    throw "Release manifest version '$($manifest.version)' does not match '$ExpectedVersion'."
}
if ([string]$manifest.commit -ne $ExpectedCommit) {
    throw "Release manifest commit '$($manifest.commit)' does not match '$ExpectedCommit'."
}

$installers = @($manifest.assets | Where-Object { [System.IO.Path]::GetExtension([string]$_.name) -eq '.exe' })
if ($installers.Count -ne 1) {
    throw "Expected exactly one Windows EXE installer in the release manifest; found $($installers.Count)."
}

$installer = $installers[0]
$installerName = [string]$installer.name
if ([System.IO.Path]::GetFileName($installerName) -ne $installerName) {
    throw "Release manifest installer name is not a file name: $installerName"
}
$installerPath = Join-Path (Split-Path -Parent $manifestFile) $installerName
if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) {
    throw "Manifest-listed Windows installer does not exist: $installerPath"
}
$actualHash = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actualHash -ne [string]$installer.sha256) {
    throw "Windows installer hash '$actualHash' does not match release manifest '$($installer.sha256)'."
}

$replacements = [ordered]@{
    '{{RELEASE_COMMIT}}' = [string]$manifest.commit
    '{{GITHUB_ACTIONS_RUN_URL}}' = $RunUrl
    '{{WINDOWS_INSTALLER_ASSET}}' = $installerName
    '{{WINDOWS_INSTALLER_SHA256}}' = $actualHash
}

$notes = Get-Content -Raw -LiteralPath $source
foreach ($entry in $replacements.GetEnumerator()) {
    if (-not $notes.Contains($entry.Key)) {
        throw "Release notes are missing required evidence token '$($entry.Key)'."
    }
    if ([string]::IsNullOrWhiteSpace($entry.Value)) {
        throw "Release evidence for '$($entry.Key)' is empty."
    }
    $notes = $notes.Replace($entry.Key, $entry.Value)
}

if ($notes.Contains('PENDING-BEFORE-TAG') -or $notes.Contains('{{RELEASE_') -or $notes.Contains('{{GITHUB_') -or $notes.Contains('{{WINDOWS_')) {
    throw 'Generated public release notes still contain an unresolved evidence marker.'
}

$parent = Split-Path -Parent $output
if (-not (Test-Path -LiteralPath $parent)) {
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
}
$notes | Set-Content -LiteralPath $output -Encoding utf8NoBOM

[pscustomobject]@{
    output = $output
    commit = [string]$manifest.commit
    installer = [string]$installer.name
    sha256 = $actualHash
    run_url = $RunUrl
} | Format-List
