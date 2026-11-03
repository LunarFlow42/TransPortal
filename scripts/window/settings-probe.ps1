param([switch]$Frontend, [switch]$CorruptOpacity, [switch]$ThemeError)
$ErrorActionPreference = 'Stop'
$example = if ($Frontend) { 'settings_frontend_probe' } else { 'settings_window_probe' }
if (($CorruptOpacity -or $ThemeError) -and -not $Frontend) { throw 'Fault injection requires -Frontend.' }
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
& cargo build --manifest-path (Join-Path $repoRoot 'src-tauri/Cargo.toml') --example $example
if ($LASTEXITCODE -ne 0) { throw 'Cannot build the settings window probe.' }

# Tauri embeds its app manifest only into the main executable, not examples.
# This probe also needs Common Controls v6 for the dialog APIs it links.
$manifestTool = Get-Command mt.exe -ErrorAction SilentlyContinue
if ($manifestTool) {
    $mt = $manifestTool.Source
} else {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
    $mt = Get-ChildItem -LiteralPath $kits -Filter mt.exe -Recurse |
        Where-Object { $_.Directory.Name -eq 'x64' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}
if (-not $mt) { throw 'Windows SDK mt.exe was not found.' }
$exe = Join-Path $repoRoot "src-tauri/target/debug/examples/$example.exe"
& $mt -nologo -manifest (Join-Path $PSScriptRoot 'settings-probe.manifest') "-outputresource:$exe;#1"
if ($LASTEXITCODE -ne 0) { throw 'Cannot embed the probe manifest.' }
$probeArgs = @()
if ($CorruptOpacity) { $probeArgs += '--corrupt-opacity' }
if ($ThemeError) { $probeArgs += '--theme-error' }
& $exe @probeArgs
if ($LASTEXITCODE -ne 0) { throw 'Settings window visibility regression detected.' }
