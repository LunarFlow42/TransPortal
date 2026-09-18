param([string]$Executable)

$ErrorActionPreference = 'Stop'
$integrityRepo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $Executable) { $Executable = Join-Path $integrityRepo 'src-tauri/target/release/integrity-bundle/transportal.exe' }
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$integrityOriginalHash = (Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash
$integrityManifest = Join-Path (Split-Path -Parent $Executable) 'integrity.json'
if (-not (Test-Path -LiteralPath $integrityManifest)) { throw 'Build a sealed NSIS release first (pnpm build:release).' }
$integrityRoot = Join-Path $integrityRepo ('src-tauri/target/integrity-smoke/' + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($integrityRoot) | Out-Null
$integrityCopy = Join-Path $integrityRoot (Split-Path -Leaf $Executable)
$integrityCopyManifest = Join-Path $integrityRoot 'integrity.json'
Copy-Item -LiteralPath $Executable -Destination $integrityCopy
Copy-Item -LiteralPath $integrityManifest -Destination $integrityCopyManifest
$integrityResults = [Collections.Generic.List[object]]::new()

function Invoke-IntegrityCase([string]$Name, [int]$ExpectedExit, [string]$ExpectedStatus) {
    $integrityStdout = Join-Path $integrityRoot ($Name + '.json')
    $integrityProcess = Start-Process -FilePath $integrityCopy -ArgumentList '--check-integrity' -WindowStyle Hidden -PassThru -RedirectStandardOutput $integrityStdout -RedirectStandardError (Join-Path $integrityRoot ($Name + '.err.log'))
    try {
        if (-not $integrityProcess.WaitForExit(15000)) { throw ('Integrity probe timed out: ' + $Name) }
        if ($integrityProcess.ExitCode -ne $ExpectedExit) { throw ('Unexpected integrity exit code: ' + $Name + ' / ' + $integrityProcess.ExitCode) }
        $integrityResult = Get-Content -LiteralPath $integrityStdout -Raw | ConvertFrom-Json
        if ($integrityResult.status -ne $ExpectedStatus) { throw ('Unexpected integrity status: ' + $Name) }
        $integrityResults.Add([pscustomobject]@{ case = $Name; exit_code = $integrityProcess.ExitCode; status = $integrityResult.status; passed = $true })
    } finally {
        if (-not $integrityProcess.HasExited) { Stop-Process -Id $integrityProcess.Id }
    }
}

Invoke-IntegrityCase 'valid' 0 'verified'
$integrityStream = [IO.File]::Open($integrityCopy, [IO.FileMode]::Append, [IO.FileAccess]::Write)
try { $integrityStream.WriteByte(0x7f) } finally { $integrityStream.Dispose() }
Invoke-IntegrityCase 'modified-executable' 2 'mismatch'
Copy-Item -LiteralPath $Executable -Destination $integrityCopy -Force
Remove-Item -LiteralPath $integrityCopyManifest
Invoke-IntegrityCase 'missing-reference' 2 'missing_manifest'
if (Test-Path -LiteralPath $integrityCopyManifest) { throw 'The app must not create its own trusted baseline.' }
[IO.File]::WriteAllText($integrityCopyManifest, '{}')
Invoke-IntegrityCase 'malformed-reference' 2 'invalid_manifest'

# Exercise normal GUI startup, not only the command-line verifier. The invalid
# copy must show the native failure dialog and exit before opening Tauri windows.
Copy-Item -LiteralPath $integrityManifest -Destination $integrityCopyManifest -Force
$integrityStream = [IO.File]::Open($integrityCopy, [IO.FileMode]::Append, [IO.FileAccess]::Write)
try { $integrityStream.WriteByte(0x7f) } finally { $integrityStream.Dispose() }
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class IntegrityDialogProbe {
  private delegate bool EnumProc(IntPtr window, IntPtr parameter);
  [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc callback, IntPtr parameter);
  [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
  [DllImport("user32.dll")] private static extern IntPtr GetDlgItem(IntPtr window, int id);
  [DllImport("user32.dll")] private static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
  public static IntPtr Find(uint process) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((window, parameter) => {
      uint owner;
      GetWindowThreadProcessId(window, out owner);
      if (owner != process) return true;
      var title = new StringBuilder(256);
      GetWindowText(window, title, title.Capacity);
      if (title.ToString() != "TransPortal 完整性检查失败") return true;
      found = window;
      return false;
    }, IntPtr.Zero);
    return found;
  }
  public static void Dismiss(IntPtr window) {
    var button = GetDlgItem(window, 1);
    if (button != IntPtr.Zero) SendMessage(button, 0x00f5, IntPtr.Zero, IntPtr.Zero);
    else SendMessage(window, 0x0010, IntPtr.Zero, IntPtr.Zero);
  }
}
'@
$integrityGui = Start-Process -FilePath $integrityCopy -WindowStyle Hidden -PassThru
try {
    $integrityDeadline = [DateTime]::UtcNow.AddSeconds(15)
    $integrityDialog = [IntPtr]::Zero
    while ($integrityDialog -eq [IntPtr]::Zero -and -not $integrityGui.HasExited -and [DateTime]::UtcNow -lt $integrityDeadline) {
        $integrityDialog = [IntegrityDialogProbe]::Find($integrityGui.Id)
        if ($integrityDialog -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 100 }
    }
    if ($integrityDialog -eq [IntPtr]::Zero) { throw 'Normal startup did not show the integrity failure dialog.' }
    [IntegrityDialogProbe]::Dismiss($integrityDialog)
    $integrityGuiStopped = $integrityGui.WaitForExit(5000)
    if (-not $integrityGuiStopped -or $integrityGui.ExitCode -ne 2) { throw ('Normal startup did not stop after a failed integrity check. Exited=' + $integrityGuiStopped + ', code=' + $integrityGui.ExitCode) }
    $integrityResults.Add([pscustomobject]@{ case = 'normal-startup-blocked'; exit_code = 2; status = 'native-dialog'; passed = $true })
} finally {
    if (-not $integrityGui.HasExited) { Stop-Process -Id $integrityGui.Id }
}
if ((Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash -cne $integrityOriginalHash) { throw 'The release executable must remain untouched.' }
$integrityResults | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $integrityRoot 'report.json') -Encoding utf8
$integrityResults | ConvertTo-Json -Depth 3
Write-Output ('Integrity report: ' + (Join-Path $integrityRoot 'report.json'))
