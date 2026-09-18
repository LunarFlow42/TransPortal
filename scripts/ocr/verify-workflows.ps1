param([switch]$SkipBuild, [switch]$BuildOnly)

$ErrorActionPreference = 'Stop'
$workflowRepo = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$workflowRun = Join-Path $workflowRepo ('src-tauri/target/workflow-e2e/' + [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
[System.IO.Directory]::CreateDirectory($workflowRun) | Out-Null
$workflowConfig = Get-Content -LiteralPath (Join-Path $workflowRepo 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json -AsHashtable
$workflowConfig.identifier = 'com.transportal.workflowtest'
$workflowConfig.build.devUrl = 'http://127.0.0.1:19420'
$workflowConfig.bundle.active = $false
foreach ($workflowWindow in $workflowConfig.app.windows) {
    $workflowWindow.Remove('additionalBrowserArgs')
    $workflowWindow.dataDirectory = Join-Path $workflowRun ('webview-' + $workflowWindow.label)
}
$workflowConfig | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $workflowRun 'tauri.test.json') -Encoding utf8
$workflowOldConfig = $env:TAURI_CONFIG
$workflowOldArgs = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$workflowOldData = $env:WEBVIEW2_USER_DATA_FOLDER
$workflowNode = (Get-Command node).Source
$workflowShell = (Get-Process -Id $PID).Path
$workflowChildren = @()
$script:workflowOutputs = @()
function Start-WorkflowProcess([string]$Executable, [string[]]$Arguments, [string]$LogName) {
    # Windows Terminal can activate a new console even with SW_HIDE. Avoid
    # creating a console at all so helpers cannot steal the fixture's focus.
    $workflowInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $workflowInfo.FileName = $Executable
    foreach ($workflowArgument in $Arguments) { $workflowInfo.ArgumentList.Add($workflowArgument) }
    $workflowInfo.WorkingDirectory = $workflowRepo
    $workflowInfo.UseShellExecute = $false
    $workflowInfo.CreateNoWindow = $true
    $workflowInfo.WindowStyle = 'Hidden'
    $workflowInfo.RedirectStandardOutput = $true
    $workflowInfo.RedirectStandardError = $true
    $workflowChild = [System.Diagnostics.Process]::new()
    $workflowChild.StartInfo = $workflowInfo
    if (-not $workflowChild.Start()) { throw "Cannot start $Executable" }
    $script:workflowOutputs += @{ process = $workflowChild; output = $workflowChild.StandardOutput.ReadToEndAsync(); error = $workflowChild.StandardError.ReadToEndAsync(); name = $LogName }
    return $workflowChild
}
try {
    $env:TAURI_CONFIG = $workflowConfig | ConvertTo-Json -Depth 20 -Compress
    if (-not $SkipBuild) {
        & cargo build --manifest-path (Join-Path $workflowRepo 'src-tauri/Cargo.toml') --bin transportal
        if ($LASTEXITCODE -ne 0) { throw 'Could not build workflow test application.' }
    }
    if ($BuildOnly) { Write-Output "Prepared desktop test application: $workflowRun"; return }
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=19421 --remote-debugging-address=127.0.0.1'
    $env:WEBVIEW2_USER_DATA_FOLDER = Join-Path $workflowRun 'webview'
    $workflowVite = Start-WorkflowProcess $workflowNode @((Join-Path $workflowRepo 'node_modules/vite/bin/vite.js'), '--host', '127.0.0.1', '--port', '19420', '--strictPort') 'vite'
    $workflowChildren += $workflowVite
    $workflowFixture = Start-WorkflowProcess $workflowShell @('-NoProfile', '-STA', '-File', (Join-Path $PSScriptRoot 'workflow-source.ps1'), '-ControlDirectory', $workflowRun) 'source'
    $workflowChildren += $workflowFixture
    $workflowReady = $false
    for ($workflowAttempt = 0; $workflowAttempt -lt 100; $workflowAttempt++) {
        try { $workflowReady = (Invoke-WebRequest -Uri 'http://127.0.0.1:19420' -TimeoutSec 1).StatusCode -eq 200 } catch { }
        if ($workflowReady) { break }
        Start-Sleep -Milliseconds 100
    }
    if (-not $workflowReady) { throw 'Workflow Vite server did not start.' }
    $workflowExe = Join-Path $workflowRepo 'src-tauri/target/debug/transportal.exe'
    foreach ($workflowPhase in @('suite', 'restart')) {
        $workflowApp = Start-WorkflowProcess $workflowExe @() $workflowPhase
        $workflowChildren += $workflowApp
        & $workflowNode (Join-Path $PSScriptRoot 'verify-workflows.mjs') $workflowRun $workflowPhase
        if ($LASTEXITCODE -ne 0) { throw "Desktop verification failed in $workflowPhase. Reports: $workflowRun" }
        if (-not $workflowApp.HasExited) { Stop-Process -Id $workflowApp.Id }
        $workflowApp.WaitForExit()
        Start-Sleep -Milliseconds 300
    }
    Write-Output "Desktop workflow reports: $workflowRun"
} finally {
    if ($workflowFixture -and -not $workflowFixture.HasExited) {
        @{ id = [guid]::NewGuid().ToString(); operation = 'quit' } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $workflowRun 'command.json') -Encoding utf8
        $workflowFixture.WaitForExit(2000) | Out-Null
    }
    foreach ($workflowChild in $workflowChildren) { if (-not $workflowChild.HasExited) { Stop-Process -Id $workflowChild.Id -ErrorAction SilentlyContinue } }
    foreach ($workflowOutput in $script:workflowOutputs) {
        $workflowOutput.process.WaitForExit(2000) | Out-Null
        if ($workflowOutput.output.IsCompleted) { $workflowOutput.output.Result | Set-Content -LiteralPath (Join-Path $workflowRun ($workflowOutput.name + '.log')) -Encoding utf8 }
        if ($workflowOutput.error.IsCompleted) { $workflowOutput.error.Result | Set-Content -LiteralPath (Join-Path $workflowRun ($workflowOutput.name + '.err.log')) -Encoding utf8 }
    }
    $env:TAURI_CONFIG = $workflowOldConfig
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $workflowOldArgs
    $env:WEBVIEW2_USER_DATA_FOLDER = $workflowOldData
}
