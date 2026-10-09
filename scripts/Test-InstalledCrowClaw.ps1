[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InstallerPath,
    [Parameter(Mandatory)][string]$ExpectedInstallerSha256,
    [Parameter(Mandatory)][string]$ExpectedExecutableSha256,
    [Parameter(Mandatory)][string]$ExpectedVersion,
    [string]$PreviousInstallerPath,
    [string]$PreviousInstallerSha256,
    [string]$PreviousVersion
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# This changes installation state and temporary app-specific WebView test policy.
# Never run it in Crow's normal account,
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
$qaPreviousInstaller = $null
if ($PreviousInstallerPath -or $PreviousInstallerSha256 -or $PreviousVersion) {
    if (-not ($PreviousInstallerPath -and $PreviousInstallerSha256 -and $PreviousVersion)) { throw 'Upgrade acceptance requires all previous-installer identity fields.' }
    $qaPreviousInstaller = (Get-Item -LiteralPath $PreviousInstallerPath).FullName
    if ((Get-FileHash -LiteralPath $qaPreviousInstaller -Algorithm SHA256).Hash -ne $PreviousInstallerSha256) {
        throw 'Previous installer hash does not match its verified release manifest.'
    }
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
$qaChecks.shortcutUiVersions = @()
$qaElevated = ([Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$qaChecks.elevatedHost = $qaElevated
$qaChecks.driverConfiguration = $(if ($qaElevated) { 'app-specific-HKLM' } else { 'child-environment' })
$qaChecks.driverLaunches = @()
$qaDriverPolicy = @()
$qaDebugLaunch = 0
$qaUiCommandIndex = 0

function Set-DriverBrowserPolicy([string]$Arguments, [string]$DataFolder) {
    if (-not $qaElevated) { return }
    if ($script:qaDriverPolicy.Count) { throw 'Previous WebView driver policy was not removed.' }
    # WebView2 ignores environment and HKCU overrides in elevated hosts.
    # Only this disposable runner and this executable receive test flags:
    # https://learn.microsoft.com/microsoft-edge/webview2/concepts/security
    $qaPolicies = [ordered]@{ AdditionalBrowserArguments = $Arguments; UserDataFolder = $DataFolder }
    foreach ($qaPolicy in $qaPolicies.Keys) {
        foreach ($qaHive in @('HKLM:', 'HKCU:')) {
            $qaPolicyPath = "$qaHive\Software\Policies\Microsoft\Edge\WebView2\$qaPolicy"
            if (-not (Test-Path -LiteralPath $qaPolicyPath)) { continue }
            $qaValues = Get-ItemProperty -LiteralPath $qaPolicyPath
            foreach ($qaName in @('crowclaw-desktop.exe', 'au.com.crowloki.crowclaw', '*')) {
                if ($null -ne $qaValues.PSObject.Properties[$qaName]) { throw 'Refusing to override existing WebView driver policy.' }
            }
        }
    }
    foreach ($qaPolicy in $qaPolicies.Keys) {
        $qaPolicyPath = 'HKLM:\Software\Policies\Microsoft'
        foreach ($qaSegment in @('Edge', 'WebView2', $qaPolicy)) {
            $qaPolicyPath += "\$qaSegment"
            # Registry -Force can erase an existing key, including other apps.
            if (-not (Test-Path -LiteralPath $qaPolicyPath)) { New-Item -Path $qaPolicyPath | Out-Null }
        }
        # Record ownership before the write so a partial failure is cleaned up too.
        $script:qaDriverPolicy += @{ Path = $qaPolicyPath; Value = $qaPolicies[$qaPolicy] }
        New-ItemProperty -LiteralPath $qaPolicyPath -Name 'crowclaw-desktop.exe' -PropertyType String -Value $qaPolicies[$qaPolicy] | Out-Null
    }
}

function Remove-DriverBrowserPolicy {
    foreach ($qaPolicy in $script:qaDriverPolicy) {
        $qaValues = Get-ItemProperty -LiteralPath $qaPolicy.Path
        $qaValue = $qaValues.PSObject.Properties['crowclaw-desktop.exe']
        if ($null -eq $qaValue) { continue }
        if ($qaValue.Value -cne $qaPolicy.Value) { throw 'WebView driver policy changed outside this test; refusing to remove it.' }
        Remove-ItemProperty -LiteralPath $qaPolicy.Path -Name 'crowclaw-desktop.exe'
        if ($null -ne (Get-ItemProperty -LiteralPath $qaPolicy.Path).PSObject.Properties['crowclaw-desktop.exe']) { throw 'WebView driver policy was not removed.' }
    }
    $script:qaDriverPolicy = @()
}

function Invoke-OwnedInstaller([string]$Path, [string[]]$Arguments) {
    $qaInstallerProcess = Start-Process -FilePath $Path -ArgumentList $Arguments -WindowStyle Hidden -PassThru
    if (-not $qaInstallerProcess.WaitForExit(60000)) { throw 'Installer did not finish within 60 seconds.' }
    if ($qaInstallerProcess.ExitCode -ne 0) { throw "Installer returned $($qaInstallerProcess.ExitCode)." }
}

function Invoke-NativeUi([string[]]$Arguments) {
    if ($Arguments[0] -eq 'run-code') {
        if ($Arguments.Count -ne 2) { throw 'Expected exactly one native UI program.' }
        # npx.cmd on Windows truncates multiline inline JavaScript. Use the
        # CLI's native file input so shell quoting cannot alter the program.
        $script:qaUiCommandIndex++
        $qaProgramFile = Join-Path $qaRoot "ui-command-$qaUiCommandIndex.js"
        [IO.File]::WriteAllText($qaProgramFile, $Arguments[1], [Text.UTF8Encoding]::new($false))
        $Arguments = @('run-code', "--filename=$qaProgramFile")
    }
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

function Get-OwnedNativeProcesses {
    if (-not $qaApp) { return @() }
    $qaProcesses = @(Get-CimInstance Win32_Process)
    $qaOwnedIds = [Collections.Generic.HashSet[uint32]]::new()
    $qaOwnedIds.Add([uint32]$qaApp.Id) | Out-Null
    do {
        $qaAdded = $false
        foreach ($qaProcess in $qaProcesses) {
            if ($qaOwnedIds.Contains($qaProcess.ParentProcessId) -and $qaOwnedIds.Add($qaProcess.ProcessId)) { $qaAdded = $true }
        }
    } while ($qaAdded)
    @($qaProcesses | Where-Object { $qaOwnedIds.Contains($_.ProcessId) })
}

function Get-LaunchDiagnostic {
    if (-not $qaApp) { return @{ appFound = $false } }
    $qaApp.Refresh()
    $qaOwned = @(Get-OwnedNativeProcesses)
    return [ordered]@{
        appFound = $true
        appExited = $qaApp.HasExited
        exitCode = $(if ($qaApp.HasExited) { $qaApp.ExitCode } else { $null })
        windowObserved = $(if ($qaApp.HasExited) { $false } else { $qaApp.MainWindowHandle -ne 0 })
        processNames = @($qaOwned | Select-Object -ExpandProperty Name -Unique)
        webViewVersions = @($qaOwned | Where-Object { $_.Name -eq 'msedgewebview2.exe' -and $_.ExecutablePath } | ForEach-Object { (Get-Item -LiteralPath $_.ExecutablePath).VersionInfo.FileVersion } | Select-Object -Unique)
        webViewDebugFlagObserved = @($qaOwned | Where-Object { $_.Name -eq 'msedgewebview2.exe' -and $_.CommandLine -match '--remote-debugging-port=9227' }).Count -gt 0
    }
}

function Assert-NormalShortcutLaunch {
    param([string]$Version)
    $qaShell = New-Object -ComObject WScript.Shell
    $qaLink = $qaShell.CreateShortcut($qaShortcut)
    if ([IO.Path]::GetFullPath($qaLink.TargetPath) -ne $qaExecutable -or $qaLink.Arguments) {
        throw 'Start menu shortcut does not launch the verified installed executable.'
    }
    # Prove the ordinary shortcut/default browser profile renders the app.
    # Do not substitute a debugger launch for this user-facing requirement.
    Start-Process -FilePath $qaShortcut -WindowStyle Hidden | Out-Null
    $qaDeadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        $qaFound = @(Get-Process -Name 'crowclaw-desktop' -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $qaExecutable })
        if ($qaFound.Count -eq 1) { $script:qaApp = $qaFound[0]; break }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $qaDeadline)
    if (-not $script:qaApp) { throw 'Installed app did not launch through its Start menu shortcut.' }
    Add-Type -AssemblyName UIAutomationClient
    $qaVisibleApp = $false
    $qaDeadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        $qaApp.Refresh()
        if ($qaApp.HasExited) { throw 'Shortcut-launched app exited before showing its UI.' }
        if ($qaApp.MainWindowHandle -ne 0) {
            try {
                $qaWindow = [Windows.Automation.AutomationElement]::FromHandle($qaApp.MainWindowHandle)
                $qaContent = $qaWindow.FindFirst([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.OrCondition]::new(
                    [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'Connect a local model'),
                    [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'CrowClaw sections')
                ))
                if ($qaContent) { $qaVisibleApp = $true; break }
            } catch [Windows.Automation.ElementNotAvailableException] { }
        }
        Start-Sleep -Milliseconds 150
    } while ([DateTime]::UtcNow -lt $qaDeadline)
    if (-not $qaVisibleApp) { throw 'Start menu launch did not expose the native onboarding/workspace UI.' }
    $script:qaChecks.shortcutUiVersions += $Version
    Stop-InstalledApp
}

function Start-InstalledApp {
    $qaVersion = [string](Get-ItemProperty -LiteralPath $qaRegistration).DisplayVersion
    if ($qaVersion -notin $qaChecks.shortcutUiVersions) { Assert-NormalShortcutLaunch $qaVersion }
    # CDP gets a scoped test configuration and a separate browser cache.
    # SQLite still uses the real default installed-app data location.
    $script:qaDebugLaunch++
    $qaStart = [Diagnostics.ProcessStartInfo]::new($qaExecutable)
    $qaStart.UseShellExecute = $false
    $qaStart.CreateNoWindow = $true
    $qaStart.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    $qaStart.WorkingDirectory = $qaInstall
    $qaDriverArguments = '--remote-debugging-port=9227 --remote-debugging-address=127.0.0.1'
    $qaDriverData = Join-Path $qaRoot "webview-driver-$qaDebugLaunch"
    if ($qaElevated) {
        Set-DriverBrowserPolicy $qaDriverArguments $qaDriverData
        $qaStart.Environment.Remove('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS') | Out-Null
        $qaStart.Environment.Remove('WEBVIEW2_USER_DATA_FOLDER') | Out-Null
    } else {
        $qaStart.Environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = $qaDriverArguments
        $qaStart.Environment['WEBVIEW2_USER_DATA_FOLDER'] = $qaDriverData
    }
    $script:qaApp = [Diagnostics.Process]::Start($qaStart)
    Wait-ForEndpoint 'http://127.0.0.1:9227/json/version'
    $qaLaunch = Get-LaunchDiagnostic
    if (-not $qaLaunch.webViewDebugFlagObserved) { throw 'CDP endpoint is not owned by the requested native WebView launch.' }
    $qaOwnedIds = @(Get-OwnedNativeProcesses | Select-Object -ExpandProperty ProcessId)
    $qaListeners = @(Get-NetTCPConnection -LocalPort 9227 -State Listen)
    if (-not $qaListeners.Count -or @($qaListeners | Where-Object { $_.LocalAddress -notin @('127.0.0.1','::1') -or $_.OwningProcess -notin $qaOwnedIds }).Count) { throw 'WebView debugger is not confined to the owned loopback listener.' }
    $script:qaChecks.driverLaunches += $qaLaunch
    Invoke-NativeUi @('attach', '--cdp=http://127.0.0.1:9227')
    Invoke-NativeUi @('snapshot')
    Invoke-NativeUi @('run-code', 'async (page) => { if(!page.url().startsWith("http://tauri.localhost/") || !await page.evaluate(() => Boolean(window.__TAURI_INTERNALS__))) throw new Error("The installed native Tauri WebView was not attached"); }')
}

function Stop-InstalledApp {
    if ($script:qaApp -and -not $script:qaApp.HasExited) {
        if ($script:qaApp.Path -ne $qaExecutable) { throw 'Acceptance process identity changed.' }
        if (-not $script:qaApp.CloseMainWindow() -or -not $script:qaApp.WaitForExit(10000)) {
            throw 'Installed app did not close normally.'
        }
    }
    $script:qaApp = $null
    Remove-DriverBrowserPolicy
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
    param([ValidateSet('user_note','legacy_crowquant')][string]$NoteKind = 'user_note', [bool]$ExpectUpgradeChoice = $false)
    $qaProgram = @'
async (page) => {
  await page.getByRole("button", {name:"Chat", exact:true}).click();
  const sidebar = page.getByRole("complementary", {name:"Conversations", exact:true});
  for (const text of ["CI telescope baseline", "CI recipe baseline"]) {
    const entry = sidebar.getByRole("button").filter({has:page.getByText(text,{exact:true})});
    if (await entry.count() !== 1) throw new Error("Expected one distinct saved conversation for " + text);
    await entry.click();
    const messages = page.getByRole("main").locator(".message__body");
    await messages.getByText(text, {exact:true}).waitFor();
    await messages.getByText("CrowClaw acceptance response: " + text, {exact:true}).waitFor();
  }
  await page.getByRole("button", {name:"Memory",exact:true}).click();
  const panel = page.locator("[aria-labelledby=native-memory-title]");
  await panel.getByRole("checkbox", {name:"Index CrowClaw conversations",exact:true}).waitFor();
  const choice = panel.getByRole("button", {name:"Index my CrowClaw conversations",exact:true});
  const choices = await choice.count();
  if (__EXPECT_UPGRADE__) {
    if (choices !== 1) throw new Error("Upgrade did not present the historical indexing choice");
    await choice.click();
  } else if (choices !== 0) {
    throw new Error("Persisted indexing consent was lost or fresh initialization was incorrect");
  }
  await panel.getByRole("checkbox", {name:"Index CrowClaw conversations",exact:true,checked:true}).waitFor();
  await panel.getByRole("combobox", {name:"Search method",exact:true}).selectOption("full_text");
  for (const [query,note,other] of [
    ["cobalt","CI native telescope cobalt record","CI native garden tulip record"],
    ["tulip","CI native garden tulip record","CI native telescope cobalt record"]
  ]) {
    await panel.getByRole("textbox", {name:"Search previous context",exact:true}).fill(query);
    await panel.getByRole("combobox", {name:"Source",exact:true}).selectOption("__NOTE_KIND__");
    await panel.getByRole("button", {name:"Search context",exact:true}).click();
    await panel.getByText(note, {exact:true}).waitFor();
    if (await panel.getByText(other, {exact:true}).count()) throw new Error("Unrelated note matched keyword query");
  }
  await panel.getByRole("textbox", {name:"Search previous context",exact:true}).fill("telescope");
  await panel.getByRole("combobox", {name:"Source",exact:true}).selectOption("conversation_message");
  await panel.getByRole("button", {name:"Search context",exact:true}).click();
  const result = panel.locator("article").filter({has:page.locator("p").filter({hasText:/^CI telescope baseline$/})});
  await result.waitFor();
  if (await result.count() !== 1) throw new Error("Expected exactly one original user-message result");
  await result.getByText("Conversation · user", {exact:true}).waitFor();
}
'@
    Invoke-NativeUi @('run-code', $qaProgram.Replace('__NOTE_KIND__', $NoteKind).Replace('__EXPECT_UPGRADE__', $ExpectUpgradeChoice.ToString().ToLowerInvariant()))
}

function Assert-InstalledMemoryApproval {
    param([ValidateSet('user_note','legacy_crowquant')][string]$NoteKind)
    $qaProgram = @'
async (page) => {
  const audit = async () => {
    const response = await page.request.get("http://127.0.0.1:32123/__acceptance/memory");
    if (!response.ok()) throw new Error("Model-side memory observer is unavailable");
    return response.json();
  };
  await page.getByRole("button", {name:"Chat",exact:true}).click();
  for (const [prompt,button,expectedDenied,expectedApproved] of [
    ["PACKAGED MEMORY DENY","Deny",0,0],
    ["PACKAGED MEMORY APPROVE","Approve once",1,0]
  ]) {
    await page.getByRole("button", {name:"New conversation",exact:true}).click();
    await page.getByRole("main").getByRole("heading", {name:"New conversation",exact:true}).waitFor();
    await page.getByRole("textbox", {name:"Message CrowClaw",exact:true}).fill(prompt);
    await page.getByRole("button", {name:"Send message",exact:true}).click();
    const approval = page.getByRole("alertdialog");
    await approval.waitFor();
    if (!(await approval.innerText()).includes("cobalt")) throw new Error("Approval omitted the actual memory query");
    const before = await audit();
    if (before.violations || before.deniedWithoutDisclosure !== expectedDenied || before.approvedSources.length !== expectedApproved) throw new Error("Memory result reached the model before the decision");
    await approval.getByRole("button", {name:button,exact:true}).click();
    const messages = page.getByRole("main").locator(".message__body");
    if (button === "Deny") {
      await messages.getByText("You denied searching CrowQuant memory. No stored memory was read.", {exact:true}).waitFor();
      if (await messages.getByText("CI native telescope cobalt record", {exact:false}).count()) throw new Error("Denied conversation contains the retained note");
    } else {
      await messages.filter({hasText:"CI native telescope cobalt record"}).waitFor();
    }
  }
  const evidence = await audit();
  if (evidence.violations || evidence.deniedWithoutDisclosure !== 1 || evidence.approvedSources.length !== 1) throw new Error("Expected one denied and one approved memory read");
  if (evidence.approvedSources[0].sourceKind !== "__NOTE_KIND__") throw new Error("Approved result lost its source kind");
}
'@
    Invoke-NativeUi @('run-code', $qaProgram.Replace('__NOTE_KIND__', $NoteKind))
    $script:qaChecks.agentMemoryDenialAndApproval = $true
    $script:qaChecks.modelMemoryEvidence = Invoke-RestMethod -Uri 'http://127.0.0.1:32123/__acceptance/memory' -TimeoutSec 3
    Invoke-NativeUi @('screenshot', ('--filename=' + (Join-Path $qaEvidence 'installed-memory-approval.png')))
}

function Assert-InstalledSemanticRecall {
    param([ValidateSet('user_note','legacy_crowquant')][string]$NoteKind)
    $qaProgram = @'
async (page) => {
  await page.getByRole("button",{name:"Memory",exact:true}).click();
  const panel=page.locator("[aria-labelledby=native-memory-title]");
  await panel.getByRole("combobox",{name:"Source",exact:true}).selectOption("__NOTE_KIND__");
  const search=async(mode,query)=>{
    await panel.getByRole("combobox",{name:"Search method",exact:true}).selectOption(mode);
    await panel.getByLabel("Search previous context",{exact:true}).fill(query);
    await panel.getByRole("button",{name:"Search context",exact:true}).click();
    await panel.locator("article.memory-card").first().getByText("CI native telescope cobalt record",{exact:true}).waitFor();
  };
  await search("lexical","cobalt");
  await panel.locator("article.memory-card").first().getByText("Lexical match",{exact:true}).waitFor();
  await panel.getByLabel("Enable a local embedding profile",{exact:true}).check();
  await panel.getByRole("combobox",{name:"Embedding protocol",exact:true}).selectOption("openai");
  await panel.getByLabel("Local embedding endpoint",{exact:true}).fill("http://127.0.0.1:32123/v1");
  await panel.getByLabel("Embedding model identifier",{exact:true}).fill("crowclaw-acceptance-model");
  await panel.getByLabel("Model output dimensions",{exact:true}).fill("3");
  await panel.getByRole("button",{name:"Save semantic profile",exact:true}).click();
  await panel.getByText("Local memory settings saved.",{exact:true}).waitFor();
  await panel.getByRole("button",{name:"Update semantic index",exact:true}).click();
  await panel.getByText(/^Stored \d+ semantic vectors;/).waitFor();
  await search("semantic","distant galaxy observation");
  await panel.locator("article.memory-card").first().getByText("Semantic match",{exact:true}).waitFor();
}
'@
    Invoke-NativeUi @('run-code',$qaProgram.Replace('__NOTE_KIND__',$NoteKind))
    $script:qaChecks.lexicalAndSemanticRecall=$true
}

function Assert-InstalledSemanticFallback {
    param([ValidateSet('user_note','legacy_crowquant')][string]$NoteKind)
    $qaProgram = @'
async (page) => {
  const panel=page.locator("[aria-labelledby=native-memory-title]");
  await panel.getByRole("combobox",{name:"Source",exact:true}).selectOption("__NOTE_KIND__");
  await panel.getByRole("combobox",{name:"Search method",exact:true}).selectOption("semantic");
  await panel.getByLabel("Search previous context",{exact:true}).fill("cobalt");
  await panel.getByRole("button",{name:"Search context",exact:true}).click();
  await panel.getByText(/Used offline keyword and CrowQuant retrieval/).first().waitFor();
  await panel.locator("article.memory-card").first().getByText("CI native telescope cobalt record",{exact:true}).waitFor();
}
'@
    Invoke-NativeUi @('run-code',$qaProgram.Replace('__NOTE_KIND__',$NoteKind))
    $script:qaChecks.visibleSemanticFallback=$true
}

function Assert-InstalledWithdrawal {
    param([ValidateSet('user_note','legacy_crowquant')][string]$NoteKind,[bool]$AlreadyWithdrawn=$false)
    $qaProgram = @'
async (page) => {
  await page.getByRole("button",{name:"Memory",exact:true}).click();
  const panel=page.locator("[aria-labelledby=native-memory-title]");
  const note="CI native telescope cobalt record";
  await panel.getByRole("combobox",{name:"Source",exact:true}).selectOption("__NOTE_KIND__");
  await panel.getByRole("combobox",{name:"Search method",exact:true}).selectOption("full_text");
  await panel.getByLabel("Search previous context",{exact:true}).fill("cobalt");
  if (!__ALREADY_WITHDRAWN__) {
    await panel.getByRole("button",{name:"Search context",exact:true}).click();
    const result=panel.locator("article.memory-card").filter({has:page.getByText(note,{exact:true})});
    await result.getByRole("button",{name:"Forget from search",exact:true}).click();
    await panel.getByText("Removed from memory search. The original conversation or note is retained.",{exact:true}).waitFor();
  }
  await panel.getByRole("button",{name:"Rebuild search index",exact:true}).click();
  await panel.getByText(/^Rebuilt \d+ sources;/).waitFor();
  await panel.getByRole("button",{name:"Search context",exact:true}).click();
  await panel.getByText("No matching indexed context. Check source filters and indexing settings.",{exact:true}).waitFor();
  await page.locator("article.crowquant-card").getByText(note,{exact:true}).waitFor();
}
'@
    Invoke-NativeUi @('run-code',$qaProgram.Replace('__NOTE_KIND__',$NoteKind).Replace('__ALREADY_WITHDRAWN__',$AlreadyWithdrawn.ToString().ToLowerInvariant()))
    if ($AlreadyWithdrawn) { $script:qaChecks.withdrawalSurvivesRestart=$true }
    else { $script:qaChecks.withdrawalRetainsOriginal=$true }
}

function Assert-InstalledRegistration([string]$Version) {
    $qaReg = Get-ItemProperty -LiteralPath $qaRegistration
    if ($qaReg.DisplayVersion -ne $Version -or $qaReg.InstallLocation.Trim('"') -ne $qaInstall) {
        throw 'Installed registration does not identify the candidate.'
    }
}

function Assert-InstalledMemoryAudit {
    param([ValidateSet('user_note','legacy_crowquant')][string]$NoteKind)
    $qaObserverFile = Join-Path $qaRoot 'model-memory-observation.json'
    $qaChecks.modelMemoryEvidence | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $qaObserverFile -Encoding utf8NoBOM
    $qaAuditJson = & node (Join-Path $qaRepository 'tests\support\installed-memory-audit.mjs') (Join-Path $qaData 'crowclaw.sqlite3') $qaObserverFile $NoteKind
    if ($LASTEXITCODE -ne 0) { throw 'Installed memory action/audit evidence did not match the model observation.' }
    $script:qaChecks.agentMemoryAudit = $qaAuditJson | ConvertFrom-Json
}

function Start-ModelFixture {
    $qaOldModelPort = $env:CROWCLAW_TEST_PORT
    try {
        $env:CROWCLAW_TEST_PORT = '32123'
        $script:qaModel = Start-Process -FilePath (Get-Command node.exe).Source -ArgumentList ('"' + (Join-Path $qaRepository 'tests\support\mock-openai-server.mjs') + '"') -WorkingDirectory $qaRoot -WindowStyle Hidden -PassThru
    } finally { $env:CROWCLAW_TEST_PORT = $qaOldModelPort }
    Wait-ForEndpoint 'http://127.0.0.1:32123/v1/models'
}

function Initialize-InstalledProfile {
    Invoke-NativeUi @('run-code', 'async (page) => { await page.getByRole("heading",{name:/Your local agent/}).waitFor(); await page.getByText("Looking locally",{exact:true}).waitFor({state:"hidden"}); await page.getByText("Custom",{selector:"strong",exact:true}).click(); await page.getByLabel("Endpoint URL",{exact:true}).fill("http://127.0.0.1:32123/v1"); await page.getByLabel("Connection name",{exact:true}).fill("Installed acceptance model"); await page.getByLabel("Model name",{exact:true}).fill("crowclaw-acceptance-model"); await page.getByRole("button",{name:"Test connection",exact:true}).click(); await page.getByRole("button",{name:"Connect and open CrowClaw",exact:true}).click({timeout:20000}); await page.getByRole("navigation",{name:"CrowClaw sections",exact:true}).waitFor(); }')
    $script:qaChecks.startMenuAndOnboarding = $true
    $qaSeedProgram = @'
async (page) => {
  for (const text of ["CI telescope baseline", "CI recipe baseline"]) {
    await page.getByRole("button", {name:"New conversation",exact:true}).click();
    await page.waitForFunction(() => { const button=document.querySelector('[aria-label="New conversation"]'); return button && !button.disabled; });
    await page.getByRole("main").getByRole("heading", {name:"New conversation",exact:true}).waitFor();
    await page.getByRole("textbox", {name:"Message CrowClaw",exact:true}).fill(text);
    await page.getByRole("button", {name:"Send message",exact:true}).click();
    await page.getByRole("main").getByText("CrowClaw acceptance response: " + text, {exact:true}).waitFor({timeout:20000});
  }
  await page.getByRole("button", {name:"Memory",exact:true}).click();
  for (const note of ["CI native telescope cobalt record", "CI native garden tulip record"]) {
    await page.getByRole("textbox", {name:"Remember something",exact:true}).fill(note);
    await page.getByRole("button", {name:"Remember with CrowQuant",exact:true}).click();
    // A textarea's text is not proof that the note has been durably saved.
    await page.locator("article.crowquant-card").getByText(note, {exact:true}).waitFor();
  }
}
'@
    Invoke-NativeUi @('run-code', $qaSeedProgram)
}

function Get-CanonicalReceipt {
    $qaJson = & node (Join-Path $qaRepository 'tests\support\canonical-memory-receipt.mjs') (Join-Path $qaData 'crowclaw.sqlite3')
    if ($LASTEXITCODE -ne 0) { throw 'Could not read the owned acceptance database.' }
    $qaJson | ConvertFrom-Json
}

function Assert-NativeRuntime {
    if (-not $qaApp -or $qaApp.HasExited) { throw 'Installed app exited before runtime verification.' }
    $qaNames = @(Get-OwnedNativeProcesses | Select-Object -ExpandProperty Name -Unique)
    if ('crowclaw-desktop.exe' -notin $qaNames -or 'msedgewebview2.exe' -notin $qaNames) { throw 'Installed native app and WebView were not both observed.' }
    if (@($qaNames | Where-Object {$_ -notin @('crowclaw-desktop.exe','msedgewebview2.exe')}).Count) { throw 'Installed core started an unexpected runtime process.' }
    $script:qaChecks.nativeRuntimeProcesses = $qaNames
}

Push-Location $qaRepository
try {
    $qaNoteKind = 'user_note'
    $qaBeforeUpgrade = $null
    if ($qaPreviousInstaller) {
        Invoke-OwnedInstaller $qaPreviousInstaller @('/S', "/D=$qaInstall")
        $qaInstalled = $true
        Assert-InstalledRegistration $PreviousVersion
        Start-ModelFixture
        Start-InstalledApp
        Initialize-InstalledProfile
        Stop-InstalledApp
        $qaBeforeUpgrade = Get-CanonicalReceipt
        if ($qaBeforeUpgrade.notes -ne 2 -or $qaBeforeUpgrade.messages -lt 4 -or $qaBeforeUpgrade.conversations -lt 2) { throw 'Previous version did not store the upgrade fixtures.' }
        $qaChecks.previousInstallerHash = $PreviousInstallerSha256
        $qaChecks.upgradedFromVersion = $PreviousVersion
        $qaChecks.beforeUpgrade = $qaBeforeUpgrade
        $qaNoteKind = 'legacy_crowquant'
    }
    Invoke-OwnedInstaller $qaInstaller @('/S', "/D=$qaInstall")
    $qaInstalled = $true
    Assert-InstalledRegistration $ExpectedVersion
    $qaInstalledHash = (Get-FileHash -LiteralPath $qaExecutable -Algorithm SHA256).Hash
    $qaChecks.installedExecutableHash = $qaInstalledHash
    $qaNormalizedHash = if ($qaInstalledHash -eq $ExpectedExecutableSha256) { $qaInstalledHash } else { Get-CrowClawNormalizedNsisHash ([IO.File]::ReadAllBytes($qaExecutable)) }
    if ($qaNormalizedHash -ne $ExpectedExecutableSha256) { throw 'Installed executable differs from the built executable.' }
    $qaChecks.normalizedExecutableMatchesBuild = $true
    $qaChecks.installedIdentity = $true
    if (-not $qaPreviousInstaller) { Start-ModelFixture }
    Start-InstalledApp
    if (-not $qaPreviousInstaller) { Initialize-InstalledProfile }
    Assert-InstalledContext -NoteKind $qaNoteKind -ExpectUpgradeChoice ([bool]$qaPreviousInstaller)
    Assert-NativeRuntime
    $qaChecks.twoConversationsAndLocalNotes = $true
    Invoke-NativeUi @('screenshot', '--filename=output/playwright/installed-memory.png')
    Stop-InstalledApp
    if ($qaBeforeUpgrade) {
        $qaAfterUpgrade = Get-CanonicalReceipt
        if ($qaBeforeUpgrade.canonicalSha256 -ne $qaAfterUpgrade.canonicalSha256 -or $qaAfterUpgrade.schemaVersion -le $qaBeforeUpgrade.schemaVersion) { throw 'Upgrade changed canonical data or did not advance the schema.' }
        $qaChecks.afterUpgrade = $qaAfterUpgrade
        $qaChecks.upgradePreservesCanonicalData = $true
    }
    if (-not $qaModel.HasExited) { $qaModel.Kill(); $qaModel.WaitForExit(5000) | Out-Null }
    $qaModel = $null

    Start-InstalledApp
    Assert-InstalledContext -NoteKind $qaNoteKind
    Assert-NativeRuntime
    $qaChecks.restartAndOfflineRecall = $true
    Invoke-NativeUi @('screenshot', '--filename=output/playwright/installed-memory-after-restart.png')
    Start-ModelFixture
    Assert-InstalledMemoryApproval -NoteKind $qaNoteKind
    Assert-InstalledMemoryAudit -NoteKind $qaNoteKind
    Assert-InstalledSemanticRecall -NoteKind $qaNoteKind
    if (-not $qaModel.HasExited) { $qaModel.Kill(); $qaModel.WaitForExit(5000) | Out-Null }
    $qaModel = $null
    Assert-InstalledSemanticFallback -NoteKind $qaNoteKind
    Assert-InstalledWithdrawal -NoteKind $qaNoteKind
    Stop-InstalledApp
    Start-InstalledApp
    Assert-InstalledWithdrawal -NoteKind $qaNoteKind -AlreadyWithdrawn $true
    Assert-NativeRuntime
    Stop-InstalledApp
    $qaBefore = Get-DataHashes
    if (-not [IO.Path]::GetFullPath($qaUninstaller).StartsWith($qaRoot + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Uninstaller escaped its owned directory.' }
    # Use normal NSIS self-removal. _?= suppresses its temporary copy and can
    # leave the running uninstaller behind, unlike the ordinary user workflow.
    Invoke-OwnedInstaller $qaUninstaller @('/S')
    $qaRemovalTargets = @($qaExecutable,$qaUninstaller,$qaRegistration,$qaShortcut,$qaDesktopShortcut)
    $qaRemovalDeadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        $qaRemaining = @($qaRemovalTargets | Where-Object { Test-Path -LiteralPath $_ })
        if (-not $qaRemaining.Count) { break }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $qaRemovalDeadline)
    if ($qaRemaining.Count) { throw 'Uninstall did not remove its binaries, registration and shortcuts.' }
    $qaInstalled = $false
    $qaAfter = Get-DataHashes
    if (($qaBefore | ConvertTo-Json -Compress) -ne ($qaAfter | ConvertTo-Json -Compress)) { throw 'Uninstall altered retained database files.' }
    $qaChecks.uninstallRetainsDatabase = $true
    $qaChecks.passed = $true
} catch {
    $qaFailure = $_
    $qaChecks.failure = $qaFailure.Exception.Message
    try { $qaChecks.launchDiagnostic = Get-LaunchDiagnostic }
    catch { $qaChecks.launchDiagnostic = @{diagnosticUnavailable = $true} }
    throw $qaFailure
} finally {
    try { Stop-InstalledApp } catch { Write-Warning $_.Exception.Message }
    $qaPolicyFailure = $null
    try { Remove-DriverBrowserPolicy; $qaChecks.driverPolicyRemoved = $true }
    catch { $qaPolicyFailure = $_; $qaChecks.driverPolicyRemoved = $false; $qaChecks.passed = $false }
    if ($qaModel -and -not $qaModel.HasExited) { $qaModel.Kill(); $qaModel.WaitForExit(5000) | Out-Null }
    # Preserve failed installation/profile evidence for the disposable runner;
    # do not fabricate uninstall acceptance by removing files manually.
    $qaChecks.installationRemains = $qaInstalled
    $qaChecks | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $qaEvidence 'installed-acceptance.json') -Encoding utf8NoBOM
    Pop-Location
    if ($qaPolicyFailure) { throw $qaPolicyFailure }
}
