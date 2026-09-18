param([string]$Language = 'en-US')

$ErrorActionPreference = 'Stop'
$ocrRepoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$ocrFixtureDir = Join-Path $ocrRepoRoot 'src-tauri/target/ocr-smoke'
[System.IO.Directory]::CreateDirectory($ocrFixtureDir) | Out-Null
$ocrFixturePath = Join-Path $ocrFixtureDir 'english.rgba'

# Generate only synthetic text. The probe never touches the user's screen or clipboard.
Add-Type -AssemblyName System.Drawing
$ocrWidth = 900
$ocrHeight = 180
$ocrBitmap = [System.Drawing.Bitmap]::new($ocrWidth, $ocrHeight, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$ocrGraphics = [System.Drawing.Graphics]::FromImage($ocrBitmap)
$ocrFont = [System.Drawing.Font]::new('Segoe UI', 28, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
try {
    $ocrGraphics.Clear([System.Drawing.Color]::White)
    $ocrGraphics.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
    $ocrGraphics.DrawString('Local OCR smoke test.', $ocrFont, [System.Drawing.Brushes]::Black, 24, 24)
    $ocrGraphics.DrawString('Translate this screenshot.', $ocrFont, [System.Drawing.Brushes]::Black, 24, 80)
    $ocrRectangle = [System.Drawing.Rectangle]::new(0, 0, $ocrWidth, $ocrHeight)
    $ocrData = $ocrBitmap.LockBits($ocrRectangle, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
        $ocrBytes = [byte[]]::new($ocrWidth * $ocrHeight * 4)
        [System.Runtime.InteropServices.Marshal]::Copy($ocrData.Scan0, $ocrBytes, 0, $ocrBytes.Length)
        for ($ocrOffset = 0; $ocrOffset -lt $ocrBytes.Length; $ocrOffset += 4) {
            $ocrBlue = $ocrBytes[$ocrOffset]
            $ocrBytes[$ocrOffset] = $ocrBytes[$ocrOffset + 2]
            $ocrBytes[$ocrOffset + 2] = $ocrBlue
        }
        [System.IO.File]::WriteAllBytes($ocrFixturePath, $ocrBytes)
    } finally {
        $ocrBitmap.UnlockBits($ocrData)
    }
} finally {
    $ocrFont.Dispose()
    $ocrGraphics.Dispose()
    $ocrBitmap.Dispose()
}

Push-Location (Join-Path $ocrRepoRoot 'src-tauri')
try {
    $ocrOutput = & cargo run --quiet --example ocr_probe -- $ocrFixturePath $ocrWidth $ocrHeight $Language
    if ($LASTEXITCODE -ne 0) { throw 'Native OCR probe failed.' }
    $ocrResult = ($ocrOutput -join "`n") | ConvertFrom-Json
    if ($ocrResult.text -notmatch 'Local OCR smoke test\.' -or $ocrResult.text -notmatch 'Translate this screenshot\.') {
        throw "Unexpected OCR result: $($ocrResult.text)"
    }
    $ocrResult | ConvertTo-Json
} finally {
    Pop-Location
}
