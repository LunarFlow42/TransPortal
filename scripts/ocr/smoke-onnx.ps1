param([switch]$Release)

$ErrorActionPreference = 'Stop'
$ocrRepoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$ocrFixtureRoot = Join-Path $ocrRepoRoot 'src-tauri/tests/fixtures/ocr'
$ocrCases = Get-Content -LiteralPath (Join-Path $ocrFixtureRoot 'cases.json') -Raw -Encoding utf8 | ConvertFrom-Json
$ocrProfile = if ($Release) { 'release' } else { 'debug' }
$ocrCargoArgs = @('build', '--manifest-path', (Join-Path $ocrRepoRoot 'src-tauri/Cargo.toml'), '--example', 'ocr_probe')
if ($Release) { $ocrCargoArgs += '--release' }
& cargo @ocrCargoArgs
if ($LASTEXITCODE -ne 0) { throw 'Cannot build the ONNX probe.' }
$ocrProbe = Join-Path $ocrRepoRoot "src-tauri/target/$ocrProfile/examples/ocr_probe.exe"
& $ocrProbe prepare
if ($LASTEXITCODE -ne 0) { throw 'Cannot prepare ONNX assets.' }

$ocrResults = foreach ($ocrCase in $ocrCases) {
    $ocrOutput = & $ocrProbe --engine onnx --image (Join-Path $ocrFixtureRoot ($ocrCase.name + '.png')) --repeat 3
    if ($LASTEXITCODE -ne 0) { throw ('ONNX inference failed: ' + $ocrCase.name) }
    $ocrResult = ($ocrOutput -join "`n") | ConvertFrom-Json
    $ocrExpected = $ocrCase.lines -join "`n"
    if (($ocrResult.text -replace '\s', '') -cne ($ocrExpected -replace '\s', '')) {
        throw "OCR mismatch in $($ocrCase.name): expected [$ocrExpected], got [$($ocrResult.text)]"
    }
    if ($ocrCase.lines.Count -gt 1 -and ($ocrResult.text -split "`n").Count -ne $ocrCase.lines.Count) {
        throw ('OCR lost line breaks: ' + $ocrCase.name)
    }
    if ($ocrExpected -and $ocrResult.blocks.Count -eq 0) { throw ('OCR lost image coordinates: ' + $ocrCase.name) }
    foreach ($ocrBlock in $ocrResult.blocks) {
        if ($ocrBlock.width -le 0 -or $ocrBlock.height -le 0 -or $ocrBlock.x -lt 0 -or $ocrBlock.y -lt 0 -or
            $ocrBlock.x + $ocrBlock.width -gt $ocrResult.width -or $ocrBlock.y + $ocrBlock.height -gt $ocrResult.height) {
            throw ('OCR returned invalid image coordinates: ' + $ocrCase.name)
        }
    }
    [PSCustomObject]@{case=$ocrCase.name;engine=$ocrResult.engine;text=$ocrResult.text;runs_ms=$ocrResult.runs_ms;passed=$true}
}
$ocrReportRoot = Join-Path $ocrRepoRoot 'src-tauri/target/ocr-smoke'
[System.IO.Directory]::CreateDirectory($ocrReportRoot) | Out-Null
$ocrResults | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $ocrReportRoot "onnx-$ocrProfile.json") -Encoding utf8
$ocrResults | ConvertTo-Json -Depth 4
