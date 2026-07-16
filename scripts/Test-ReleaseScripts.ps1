$ErrorActionPreference = 'Stop'
$root = [System.IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("crowclaw-release-script-test-" + [guid]::NewGuid().ToString('N'))

try {
    $bundle = Join-Path $fixtureRoot 'src-tauri\target\release\bundle'
    New-Item -ItemType Directory -Path (Join-Path $bundle 'nsis') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $bundle 'msi') -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $fixtureRoot 'package.json') -Value '{"version":"0.1.0-test"}' -Encoding utf8NoBOM
    New-Item -ItemType Directory -Path (Join-Path $fixtureRoot '.git') | Out-Null
    Copy-Item -LiteralPath (Join-Path $root 'scripts\Collect-ReleaseArtifacts.ps1') -Destination (Join-Path $fixtureRoot 'Collect.ps1')
    Set-Content -LiteralPath (Join-Path $bundle 'nsis\CrowClaw-test.exe') -Value 'nsis-fixture' -Encoding ascii
    Set-Content -LiteralPath (Join-Path $bundle 'msi\CrowClaw-test.msi') -Value 'msi-fixture' -Encoding ascii

    $fakeGit = Join-Path $fixtureRoot 'git.cmd'
    Set-Content -LiteralPath $fakeGit -Value '@echo 0123456789abcdef0123456789abcdef01234567' -Encoding ascii
    $oldPath = $env:PATH
    $env:PATH = "$fixtureRoot;$oldPath"
    try {
        & (Join-Path $fixtureRoot 'Collect.ps1') -RepositoryRoot $fixtureRoot -OutputDirectory (Join-Path $fixtureRoot 'release')
    } finally {
        $env:PATH = $oldPath
    }

    $manifest = Get-Content -Raw -LiteralPath (Join-Path $fixtureRoot 'release\release-manifest.json') | ConvertFrom-Json
    if ($manifest.assets.Count -ne 2) { throw 'Expected two collected installer assets' }
    if (-not (Test-Path -LiteralPath (Join-Path $fixtureRoot 'release\SHA256SUMS.txt'))) { throw 'Missing checksum file' }

    $notesSource = Join-Path $fixtureRoot 'release-notes-source.md'
    $notesOutput = Join-Path $fixtureRoot 'public-release-notes.md'
    @'
Commit: {{RELEASE_COMMIT}}
Run: {{GITHUB_ACTIONS_RUN_URL}}
Installer: {{WINDOWS_INSTALLER_ASSET}}
SHA-256: {{WINDOWS_INSTALLER_SHA256}}
'@ | Set-Content -LiteralPath $notesSource -Encoding utf8NoBOM
    & (Join-Path $root 'scripts\New-PublicReleaseNotes.ps1') `
        -SourcePath $notesSource `
        -ManifestPath (Join-Path $fixtureRoot 'release\release-manifest.json') `
        -OutputPath $notesOutput `
        -RunUrl 'https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/123456' `
        -ExpectedVersion '0.1.0-test' `
        -ExpectedCommit '0123456789abcdef0123456789abcdef01234567'

    $publicNotes = Get-Content -Raw -LiteralPath $notesOutput
    if ($publicNotes.Contains('{{')) { throw 'Public release notes retain an unresolved token' }
    if (-not $publicNotes.Contains('0123456789abcdef0123456789abcdef01234567')) { throw 'Public release notes omit the manifest commit' }
    if (-not $publicNotes.Contains('CrowClaw-test.exe')) { throw 'Public release notes omit the Windows installer name' }
    if (-not $publicNotes.Contains([string]($manifest.assets | Where-Object name -eq 'CrowClaw-test.exe').sha256)) { throw 'Public release notes omit the Windows installer hash' }
    'Release script tests passed.'
} finally {
    if (Test-Path -LiteralPath $fixtureRoot) {
        $resolved = [System.IO.Path]::GetFullPath($fixtureRoot)
        $temp = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
        if (-not $resolved.StartsWith($temp, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove test directory outside TEMP: $resolved"
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
