[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InstallerPath,
    [Parameter(Mandatory)][string]$ExpectedInstallerSha256,
    [Parameter(Mandatory)][string]$ExpectedExecutableSha256,
    [Parameter(Mandatory)][string]$ExpectedVersion
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# This changes HKCU installation state. Never run it in Crow's normal account,
# a self-hosted runner, or over an existing installation/profile/shortcut.
if (-not $IsWindows -or $PSVersionTable.PSVersion.Major -lt 7 -or
    $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or
    $env:RUNNER_OS -cne 'Windows' -or [string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
    throw 'Installed acceptance requires a fresh GitHub-hosted Windows runner and PowerShell 7.'
}
. (Join-Path $PSScriptRoot 'InstallerIdentity.ps1')

$qaRepository = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$qaRegistration = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CrowClaw'
$qaMachineRegistration = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CrowClaw'
$qaData = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'au.com.crowloki.crowclaw'
$qaShortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'CrowClaw.lnk'
$qaDesktopShortcut = Join-Path ([Environment]::GetFolderPath('DesktopDirectory')) 'CrowClaw.lnk'
$qaExistingPaths = @(
    $qaRegistration, $qaMachineRegistration, $qaData, $qaShortcut, $qaDesktopShortcut,
    (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'CrowClaw'),
    (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'au.com.crowloki.crowclaw'),
    (Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'CrowClaw.lnk'),
    (Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'CrowClaw'),
    (Join-Path ([Environment]::GetFolderPath('CommonDesktopDirectory')) 'CrowClaw.lnk'),
    (Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'CrowClaw'),
    (Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) 'CrowClaw'),
    'HKCU:\Software\CrowClaw\CrowClaw'
)
foreach ($qaExisting in $qaExistingPaths) {
    if (Test-Path -LiteralPath $qaExisting) { throw 'Refusing installed acceptance: CrowClaw state already exists.' }
}
foreach ($qaRegistryRoot in @(
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKCU:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
)) {
    if (-not (Test-Path -LiteralPath $qaRegistryRoot)) { continue }
    foreach ($qaKey in Get-ChildItem -LiteralPath $qaRegistryRoot) {
        $qaInstalledEntry = Get-ItemProperty -LiteralPath $qaKey.PSPath
        $qaDisplayName = $qaInstalledEntry.PSObject.Properties['DisplayName']
        if ($qaDisplayName -and [string]$qaDisplayName.Value -match '^CrowClaw(\s|$)') {
            throw 'Refusing installed acceptance: an existing CrowClaw NSIS/MSI registration was found.'
        }
    }
}
if (Get-Process -Name 'crowclaw-desktop' -ErrorAction SilentlyContinue) {
    throw 'Refusing installed acceptance: CrowClaw is already running.'
}
$qaInstaller = (Get-Item -LiteralPath $InstallerPath).FullName
if ((Get-FileHash -LiteralPath $qaInstaller -Algorithm SHA256).Hash -ne $ExpectedInstallerSha256) {
    throw 'Installer hash does not match the collected artifact.'
}
foreach ($qaPort in @(32123, 9227)) {
    if (Get-NetTCPConnection -LocalPort $qaPort -State Listen -ErrorAction SilentlyContinue) {
        throw "Acceptance port $qaPort is already occupied."
    }
}

$qaRunnerTemp = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
$qaRoot = [IO.Path]::GetFullPath((Join-Path $qaRunnerTemp ('CrowClaw-acceptance-' + [guid]::NewGuid())))
if (-not $qaRoot.StartsWith($qaRunnerTemp, [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid acceptance root.' }
New-Item -ItemType Directory -Path $qaRoot | Out-Null
$qaInstall = Join-Path $qaRoot 'app'
$qaExecutable = Join-Path $qaInstall 'crowclaw-desktop.exe'
$qaUninstaller = Join-Path $qaInstall 'uninstall.exe'
$qaEvidence = Join-Path $qaRepository 'output\playwright'
New-Item -ItemType Directory -Path $qaEvidence -Force | Out-Null
$qaApp = $null
$qaModel = $null
$qaInstalled = $false
$qaChecks = [ordered]@{ installerHash = $ExpectedInstallerSha256; executableHash = $ExpectedExecutableSha256; version = $ExpectedVersion; passed = $false }

function Invoke-OwnedInstaller([string]$Path, [string[]]$Arguments) {
    $qaInstallerProcess = Start-Process -FilePath $Path -ArgumentList $Arguments -WindowStyle Hidden -PassThru
    if (-not $qaInstallerProcess.WaitForExit(60000)) { throw 'Installer did not finish within 60 seconds.' }
    if ($qaInstallerProcess.ExitCode -ne 0) { throw "Installer returned $($qaInstallerProcess.ExitCode)." }
}

function Invoke-NativeUi([string[]]$Arguments) {
    $qaUiOutput = & npx --yes --package '@playwright/cli@0.1.22' playwright-cli -s=crowclaw-installed-ci @Arguments 2>&1
    if ($LASTEXITCODE -ne 0 -or ($qaUiOutput -match '^### Error')) {
        $qaUiOutput | Write-Output
        throw 'Installed WebView UI acceptance failed.'
    }
}

function Wait-ForEndpoint([string]$Uri) {
    $qaDeadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        try { Invoke-RestMethod -Uri $Uri -TimeoutSec 1 | Out-Null; return } catch { Start-Sleep -Milliseconds 100 }
    } while ([DateTime]::UtcNow -lt $qaDeadline)
    throw "Acceptance endpoint did not become ready: $Uri"
}

function Start-InstalledApp {
    $qaShell = New-Object -ComObject WScript.Shell
    $qaLink = $qaShell.CreateShortcut($qaShortcut)
    if ([IO.Path]::GetFullPath($qaLink.TargetPath) -ne $qaExecutable -or $qaLink.Arguments) {
        throw 'Start menu shortcut does not launch the verified installed executable.'
    }
    $qaOldArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
    try {
        $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9227 --remote-debugging-address=127.0.0.1'
        Start-Process -FilePath $qaShortcut -WindowStyle Hidden | Out-Null
    } finally { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $qaOldArguments }
    $qaDeadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        $qaFound = @(Get-Process -Name 'crowclaw-desktop' -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $qaExecutable })
        if ($qaFound.Count -eq 1) { $script:qaApp = $qaFound[0]; break }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $qaDeadline)
    if (-not $script:qaApp) { throw 'Installed app did not launch through its Start menu shortcut.' }
    Wait-ForEndpoint 'http://127.0.0.1:9227/json/version'
    Invoke-NativeUi @('attach', '--cdp=http://127.0.0.1:9227')
    Invoke-NativeUi @('snapshot')
}

function Stop-InstalledApp {
    if ($script:qaApp -and -not $script:qaApp.HasExited) {
        if ($script:qaApp.Path -ne $qaExecutable) { throw 'Acceptance process identity changed.' }
        if (-not $script:qaApp.CloseMainWindow() -or -not $script:qaApp.WaitForExit(10000)) {
            throw 'Installed app did not close normally.'
        }
    }
    $script:qaApp = $null
}

function Get-DataHashes {
    $qaHashes = [ordered]@{}
    foreach ($qaFile in @(Get-ChildItem -LiteralPath $qaData -Filter 'crowclaw.sqlite3*' -File)) {
        $qaHashes[$qaFile.Name] = (Get-FileHash -LiteralPath $qaFile.FullName -Algorithm SHA256).Hash
    }
    if (-not $qaHashes.Contains('crowclaw.sqlite3')) { throw 'The installed app did not create its own database.' }
    return $qaHashes
}

function Assert-InstalledContext {
    Invoke-NativeUi @('run-code', 'async (page) => { await page.getByRole("button",{name:"Chat",exact:true}).click(); const sidebar=page.getByRole("complementary",{name:"Conversations",exact:true}); for(const text of ["CI telescope baseline", "CI recipe baseline"]) { const entry=sidebar.getByRole("button").filter({has:page.getByText(text,{exact:true})}); if(await entry.count() !== 1) throw new Error("Expected one distinct saved conversation for " + text); await entry.click(); const messages=page.getByRole("main").locator(".message__body"); await messages.getByText(text,{exact:true}).waitFor(); await messages.getByText("CrowClaw acceptance response: " + text,{exact:true}).waitFor(); } await page.getByRole("button",{name:"Memory",exact:true}).click(); const panel=page.locator("[aria-labelledby=native-memory-title]"); await panel.getByRole("combobox",{name:"Search method",exact:true}).selectOption("full_text"); for(const [query,note,other] of [["cobalt","CI native telescope cobalt record","CI native garden tulip record"],["tulip","CI native garden tulip record","CI native telescope cobalt record"]]) { await panel.getByRole("textbox",{name:"Search previous context",exact:true}).fill(query); await panel.getByRole("combobox",{name:"Source",exact:true}).selectOption("user_note"); await panel.getByRole("button",{name:"Search context",exact:true}).click(); await panel.getByText(note,{exact:true}).waitFor(); if(await panel.getByText(other,{exact:true}).count()) throw new Error("Unrelated note matched keyword query"); } await panel.getByRole("textbox",{name:"Search previous context",exact:true}).fill("telescope"); await panel.getByRole("combobox",{name:"Source",exact:true}).selectOption("conversation_message"); await panel.getByRole("button",{name:"Search context",exact:true}).click(); await panel.getByText("CI telescope baseline",{exact:true}).waitFor(); }')
}

Push-Location $qaRepository
try {
    Invoke-OwnedInstaller $qaInstaller @('/S', "/D=$qaInstall")
    $qaInstalled = $true
    $qaReg = Get-ItemProperty -LiteralPath $qaRegistration
    if ($qaReg.DisplayVersion -ne $ExpectedVersion -or $qaReg.InstallLocation.Trim('"') -ne $qaInstall) {
        throw 'Installed registration does not identify the candidate.'
    }
    $qaInstalledHash = (Get-FileHash -LiteralPath $qaExecutable -Algorithm SHA256).Hash
    $qaChecks.installedExecutableHash = $qaInstalledHash
    $qaNormalizedHash = if ($qaInstalledHash -eq $ExpectedExecutableSha256) { $qaInstalledHash } else { Get-CrowClawNormalizedNsisHash ([IO.File]::ReadAllBytes($qaExecutable)) }
    if ($qaNormalizedHash -ne $ExpectedExecutableSha256) {
        throw 'Installed executable differs from the built executable.'
    }
    $qaChecks.normalizedExecutableMatchesBuild = $true
    $qaChecks.installedIdentity = $true

    $qaOldModelPort = $env:CROWCLAW_TEST_PORT
    try {
        $env:CROWCLAW_TEST_PORT = '32123'
        $qaModel = Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList ('"' + (Join-Path $qaRepository 'tests\support\mock-openai-server.mjs') + '"') -WorkingDirectory $qaRoot -WindowStyle Hidden -PassThru
    } finally { $env:CROWCLAW_TEST_PORT = $qaOldModelPort }
    Wait-ForEndpoint 'http://127.0.0.1:32123/v1/models'
    Start-InstalledApp
    Invoke-NativeUi @('run-code', 'async (page) => { await page.getByRole("heading",{name:/Your local agent/}).waitFor(); await page.getByText("Looking locally",{exact:true}).waitFor({state:"hidden"}); await page.getByText("Custom",{selector:"strong",exact:true}).click(); await page.getByLabel("Endpoint URL",{exact:true}).fill("http://127.0.0.1:32123/v1"); await page.getByLabel("Connection name",{exact:true}).fill("Installed acceptance model"); await page.getByLabel("Model name",{exact:true}).fill("crowclaw-acceptance-model"); await page.getByRole("button",{name:"Test connection",exact:true}).click(); await page.getByRole("button",{name:"Connect and open CrowClaw",exact:true}).click({timeout:20000}); await page.getByRole("navigation",{name:"CrowClaw sections",exact:true}).waitFor(); }')
    $qaChecks.startMenuAndOnboarding = $true
    Invoke-NativeUi @('run-code', 'async (page) => { for(const text of ["CI telescope baseline", "CI recipe baseline"]) { await page.getByRole("button",{name:"New conversation",exact:true}).click(); await page.waitForFunction(() => { const button=document.querySelector("[aria-label=\"New conversation\"]"); return button && !button.disabled; }); await page.getByRole("main").getByRole("heading",{name:"New conversation",exact:true}).waitFor(); await page.getByRole("textbox",{name:"Message CrowClaw",exact:true}).fill(text); await page.getByRole("button",{name:"Send message",exact:true}).click(); await page.getByRole("main").getByText("CrowClaw acceptance response: " + text,{exact:true}).waitFor({timeout:20000}); } await page.getByRole("button",{name:"Memory",exact:true}).click(); for(const note of ["CI native telescope cobalt record", "CI native garden tulip record"]) { await page.getByRole("textbox",{name:"Remember something",exact:true}).fill(note); await page.getByRole("button",{name:"Remember with CrowQuant",exact:true}).click(); await page.getByText(note,{exact:true}).waitFor(); } }')
    Assert-InstalledContext
    $qaChecks.twoConversationsAndLocalNotes = $true
    Invoke-NativeUi @('screenshot', '--filename=output/playwright/installed-memory.png')
    Stop-InstalledApp
    if (-not $qaModel.HasExited) { $qaModel.Kill(); $qaModel.WaitForExit(5000) | Out-Null }
    $qaModel = $null

    Start-InstalledApp
    Assert-InstalledContext
    $qaChecks.restartAndOfflineRecall = $true
    Invoke-NativeUi @('screenshot', '--filename=output/playwright/installed-memory-after-restart.png')
    Stop-InstalledApp
    $qaBefore = Get-DataHashes
    if (-not [IO.Path]::GetFullPath($qaUninstaller).StartsWith($qaRoot + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Uninstaller escaped its owned directory.' }
    Invoke-OwnedInstaller $qaUninstaller @('/S', "_?=$qaInstall")
    $qaInstalled = $false
    if ((Test-Path -LiteralPath $qaExecutable) -or (Test-Path -LiteralPath $qaRegistration) -or (Test-Path -LiteralPath $qaShortcut)) {
        throw 'Uninstall did not remove the candidate binary, registration and Start menu shortcut.'
    }
    $qaAfter = Get-DataHashes
    if (($qaBefore | ConvertTo-Json -Compress) -ne ($qaAfter | ConvertTo-Json -Compress)) { throw 'Uninstall altered retained database files.' }
    $qaChecks.uninstallRetainsDatabase = $true
    $qaChecks.passed = $true
} catch {
    $qaChecks.failure = $_.Exception.Message
    throw
} finally {
    try { Stop-InstalledApp } catch { Write-Warning $_.Exception.Message }
    if ($qaModel -and -not $qaModel.HasExited) { $qaModel.Kill(); $qaModel.WaitForExit(5000) | Out-Null }
    # Preserve failed installation/profile evidence for the disposable runner;
    # do not fabricate uninstall acceptance by removing files manually.
    $qaChecks.installationRemains = $qaInstalled
    $qaChecks | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $qaEvidence 'installed-acceptance.json') -Encoding utf8NoBOM
    Pop-Location
}
