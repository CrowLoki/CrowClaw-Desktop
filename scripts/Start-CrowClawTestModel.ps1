[CmdletBinding()]
param(
    [int]$Port = 32123
)

$ErrorActionPreference = 'Stop'
$root = [System.IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$server = Join-Path $root 'tests\support\mock-openai-server.mjs'
$logDirectory = Join-Path $root '.test-runtime'
New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null

$stdout = Join-Path $logDirectory 'model.stdout.log'
$stderr = Join-Path $logDirectory 'model.stderr.log'
$env:CROWCLAW_TEST_PORT = [string]$Port
$process = Start-Process -FilePath 'node.exe' -ArgumentList @($server) -WorkingDirectory $root -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr

$deadline = [DateTime]::UtcNow.AddSeconds(10)
do {
    Start-Sleep -Milliseconds 100
    try {
        $models = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/v1/models" -TimeoutSec 1
        [pscustomobject]@{
            pid = $process.Id
            endpoint = "http://127.0.0.1:$Port/v1"
            model = $models.data[0].id
            stdout = $stdout
            stderr = $stderr
        } | Format-List
        exit 0
    } catch {
        if ($process.HasExited) {
            throw "Test model exited early. See $stderr"
        }
    }
} while ([DateTime]::UtcNow -lt $deadline)

Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
throw 'Timed out waiting for CrowClaw test model'
