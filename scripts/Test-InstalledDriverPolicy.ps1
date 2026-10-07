$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Extract only the two policy functions. Never execute the installer harness or
# access a real registry; these command doubles keep the safety checks portable.
$qaTokens = $null
$qaErrors = $null
$qaAst = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'Test-InstalledCrowClaw.ps1'), [ref]$qaTokens, [ref]$qaErrors)
if ($qaErrors.Count) { throw 'Installed acceptance PowerShell did not parse.' }
foreach ($qaName in @('Set-DriverBrowserPolicy', 'Remove-DriverBrowserPolicy')) {
    $qaFunction = $qaAst.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $qaName}, $true)
    if (-not $qaFunction) { throw "Missing policy function: $qaName" }
    . ([scriptblock]::Create($qaFunction.Extent.Text))
}

$qaRegistry = @{}
$qaDriverPolicy = @()
$qaElevated = $true
$qaFailWrite = $false
$qaBase = 'HKLM:\Software\Policies\Microsoft\Edge\WebView2'
function Test-Path([string]$LiteralPath) { $qaRegistry.ContainsKey($LiteralPath) }
function Get-ItemProperty([string]$LiteralPath) { [pscustomobject]$qaRegistry[$LiteralPath] }
function New-Item([string]$Path, [switch]$Force) {
    # Registry New-Item -Force empties an existing key, unlike a directory.
    if ($qaRegistry.ContainsKey($Path) -and -not $Force) { throw 'Registry key already exists.' }
    $qaRegistry[$Path] = @{}
}
function New-ItemProperty([string]$LiteralPath, [string]$Name, [string]$PropertyType, [string]$Value) {
    if (-not $LiteralPath.StartsWith($qaBase + '\') -or $Name -ne 'crowclaw-desktop.exe' -or $PropertyType -ne 'String') { throw 'Policy write escaped the app-specific scope.' }
    if ($qaFailWrite -and $LiteralPath.EndsWith('\UserDataFolder')) { throw 'Simulated second write failure.' }
    $qaRegistry[$LiteralPath][$Name] = $Value
}
function Remove-ItemProperty([string]$LiteralPath, [string]$Name) { $qaRegistry[$LiteralPath].Remove($Name) }
function Assert-PolicyRejected([scriptblock]$Action) {
    $qaRejected = $false
    try { & $Action } catch { $qaRejected = $true }
    if (-not $qaRejected) { throw 'Expected policy operation to refuse.' }
}

$qaElevated = $false
Set-DriverBrowserPolicy '--remote-debugging-port=9227' 'synthetic-cache'
if ($qaRegistry.Count -or $qaDriverPolicy.Count) { throw 'Non-elevated launch wrote registry policy.' }
$qaElevated = $true
foreach ($qaHive in @('HKLM:', 'HKCU:')) {
    foreach ($qaPolicy in @('AdditionalBrowserArguments', 'UserDataFolder')) {
        foreach ($qaName in @('crowclaw-desktop.exe', 'au.com.crowloki.crowclaw', '*')) {
            $qaRegistry = @{ "$qaHive\Software\Policies\Microsoft\Edge\WebView2\$qaPolicy" = @{ $qaName = '' } }
            Assert-PolicyRejected { Set-DriverBrowserPolicy 'test-flags' 'synthetic-cache' }
            if ($qaDriverPolicy.Count -or $qaRegistry.Count -ne 1) { throw 'Pre-existing policy was changed before refusal.' }
        }
    }
}
$qaRegistry = @{ "$qaBase\AdditionalBrowserArguments" = @{ 'unrelated.exe' = 'keep' } }
Set-DriverBrowserPolicy 'test-flags' 'synthetic-cache'
if ($qaDriverPolicy.Count -ne 2) { throw 'Both driver policies were not tracked.' }
Assert-PolicyRejected { Set-DriverBrowserPolicy 'different' 'different' }
Remove-DriverBrowserPolicy
Remove-DriverBrowserPolicy
if ($qaDriverPolicy.Count -or $qaRegistry["$qaBase\AdditionalBrowserArguments"].Count -ne 1 -or $qaRegistry["$qaBase\AdditionalBrowserArguments"]['unrelated.exe'] -ne 'keep' -or $qaRegistry["$qaBase\UserDataFolder"].Count) { throw 'Cleanup lost unrelated policy or retained test policy.' }

$qaFailWrite = $true
Assert-PolicyRejected { Set-DriverBrowserPolicy 'test-flags' 'synthetic-cache' }
Remove-DriverBrowserPolicy
if ($qaRegistry["$qaBase\AdditionalBrowserArguments"].ContainsKey('crowclaw-desktop.exe')) { throw 'Partial failure left driver flags behind.' }
$qaFailWrite = $false
Set-DriverBrowserPolicy 'test-flags' 'synthetic-cache'
$qaRegistry["$qaBase\AdditionalBrowserArguments"]['crowclaw-desktop.exe'] = 'changed-externally'
Assert-PolicyRejected { Remove-DriverBrowserPolicy }
if ($qaRegistry["$qaBase\AdditionalBrowserArguments"]['crowclaw-desktop.exe'] -ne 'changed-externally') { throw 'Cleanup removed externally changed policy.' }
'Installed driver policy tests passed: scoped writes, existing-state refusal, partial-failure cleanup and unrelated-state preservation.'
