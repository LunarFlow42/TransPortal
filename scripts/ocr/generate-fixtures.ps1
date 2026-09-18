$ErrorActionPreference = 'Stop'
$ocrRepoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$ocrFixtureRoot = Join-Path $ocrRepoRoot 'src-tauri/tests/fixtures/ocr'
$ocrCases = Get-Content -LiteralPath (Join-Path $ocrFixtureRoot 'cases.json') -Raw -Encoding utf8 | ConvertFrom-Json
Add-Type -AssemblyName System.Drawing

foreach ($ocrCase in $ocrCases) {
    $ocrBitmap = [System.Drawing.Bitmap]::new($ocrCase.width, $ocrCase.height, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $ocrGraphics = [System.Drawing.Graphics]::FromImage($ocrBitmap)
    $ocrFont = [System.Drawing.Font]::new($ocrCase.font, 28, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
    try {
        $ocrBackground = switch ($ocrCase.theme) {
            'dark' { [System.Drawing.Color]::FromArgb(255, 13, 18, 32) }
            'transparent' { [System.Drawing.Color]::Transparent }
            default { [System.Drawing.Color]::White }
        }
        $ocrBrush = if ($ocrCase.theme -eq 'dark') { [System.Drawing.Brushes]::White } else { [System.Drawing.Brushes]::Black }
        $ocrGraphics.Clear($ocrBackground)
        $ocrGraphics.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        for ($ocrLine = 0; $ocrLine -lt $ocrCase.lines.Count; $ocrLine++) {
            $ocrGraphics.DrawString($ocrCase.lines[$ocrLine], $ocrFont, $ocrBrush, 24, (24 + $ocrLine * 56))
        }
        if ($ocrCase.rotate) { $ocrBitmap.RotateFlip([System.Drawing.RotateFlipType]::Rotate180FlipNone) }
        $ocrBitmap.Save((Join-Path $ocrFixtureRoot ($ocrCase.name + '.png')), [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $ocrFont.Dispose()
        $ocrGraphics.Dispose()
        $ocrBitmap.Dispose()
    }
}
Write-Output 'Generated synthetic OCR fixtures. No screen or clipboard data was used.'
