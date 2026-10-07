$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'InstallerIdentity.ps1')
$qaEncoding = [Text.Encoding]::Latin1
$qaBuilt = $qaEncoding.GetBytes('fixture-prefix __TAURI_BUNDLE_TYPE_VAR_UNK fixture-suffix')
$qaInstalled = $qaEncoding.GetBytes('fixture-prefix __TAURI_BUNDLE_TYPE_VAR_NSS fixture-suffix')
$qaExpected = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($qaBuilt))
if ((Get-CrowClawNormalizedNsisHash $qaInstalled) -ne $qaExpected) { throw 'NSIS bundle-marker-only change did not match the built bytes.' }
$qaChanged = $qaEncoding.GetBytes('different-prefix __TAURI_BUNDLE_TYPE_VAR_NSS fixture-suffix')
if ((Get-CrowClawNormalizedNsisHash $qaChanged) -eq $qaExpected) { throw 'Unrelated binary mutation was accepted.' }
foreach ($qaInvalid in @('no marker', '__TAURI_BUNDLE_TYPE_VAR_MSI', '__TAURI_BUNDLE_TYPE_VAR_NSS __TAURI_BUNDLE_TYPE_VAR_NSS', '__TAURI_BUNDLE_TYPE_VAR_NSS __TAURI_BUNDLE_TYPE_VAR_UNK')) {
    $qaRejected = $false
    try { Get-CrowClawNormalizedNsisHash ($qaEncoding.GetBytes($qaInvalid)) | Out-Null } catch { $qaRejected = $true }
    if (-not $qaRejected) { throw 'Missing, ambiguous or wrong bundle marker was accepted.' }
}
if ($qaEncoding.GetString($qaInstalled) -ne 'fixture-prefix __TAURI_BUNDLE_TYPE_VAR_NSS fixture-suffix') { throw 'Identity check modified its input.' }
$qaTokens = $null
$qaErrors = $null
$qaAst = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'Test-InstalledCrowClaw.ps1'), [ref]$qaTokens, [ref]$qaErrors)
if ($qaErrors.Count) { throw 'Installed acceptance PowerShell did not parse.' }
# The UI programs are real JavaScript passed to the CLI, not PowerShell code.
# Parse them without executing any UI, installer or native command.
$qaPrograms = $qaAst.FindAll({param($node) $node -is [Management.Automation.Language.StringConstantExpressionAst] -and $node.Value.StartsWith('async (page)')}, $true)
foreach ($qaProgram in $qaPrograms) {
    $qaProgram.Value | node --check --input-type=module
    if ($LASTEXITCODE -ne 0) { throw 'Installed acceptance UI program did not parse.' }
}
'Installer identity tests passed: exact bundle marker accepted; unrelated, missing and ambiguous changes rejected.'
