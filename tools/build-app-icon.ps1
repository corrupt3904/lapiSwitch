param(
    [string]$Source = (Join-Path $PSScriptRoot '..\crates\lapiswitch-gui\assets\branding\lapiswitch-source.png'),
    [string]$Destination = (Join-Path $PSScriptRoot '..\crates\lapiswitch-gui\assets\branding')
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Path $Destination -Force | Out-Null
$sourceBitmap = [System.Drawing.Bitmap]::new((Resolve-Path -LiteralPath $Source).Path)
try {
    if ($sourceBitmap.Width -ne $sourceBitmap.Height) { throw 'The approved icon must be square.' }
    $sizes = @(16, 24, 32, 48, 64, 128, 256)
    $frames = @()
    foreach ($size in $sizes) {
        $bitmap = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $attributes = [System.Drawing.Imaging.ImageAttributes]::new()
        $stream = [System.IO.MemoryStream]::new()
        try {
            $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
            $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $attributes.SetWrapMode([System.Drawing.Drawing2D.WrapMode]::TileFlipXY)
            $graphics.Clear([System.Drawing.Color]::Transparent)
            $rectangle = [System.Drawing.Rectangle]::new(0, 0, $size, $size)
            $graphics.DrawImage($sourceBitmap, $rectangle, 0, 0, $sourceBitmap.Width, $sourceBitmap.Height, [System.Drawing.GraphicsUnit]::Pixel, $attributes)
            $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
            $frames += ,$stream.ToArray()
            if ($size -eq 256) {
                [System.IO.File]::WriteAllBytes((Join-Path $Destination 'lapiswitch.png'), $stream.ToArray())
            }
        } finally {
            $stream.Dispose()
            $attributes.Dispose()
            $graphics.Dispose()
            $bitmap.Dispose()
        }
    }
    $iconStream = [System.IO.MemoryStream]::new()
    $writer = [System.IO.BinaryWriter]::new($iconStream)
    try {
        $writer.Write([uint16]0)
        $writer.Write([uint16]1)
        $writer.Write([uint16]$sizes.Count)
        $offset = 6 + 16 * $sizes.Count
        for ($index = 0; $index -lt $sizes.Count; $index++) {
            $dimension = if ($sizes[$index] -eq 256) { 0 } else { $sizes[$index] }
            $writer.Write([byte]$dimension)
            $writer.Write([byte]$dimension)
            $writer.Write([byte]0)
            $writer.Write([byte]0)
            $writer.Write([uint16]1)
            $writer.Write([uint16]32)
            $writer.Write([uint32]$frames[$index].Length)
            $writer.Write([uint32]$offset)
            $offset += $frames[$index].Length
        }
        foreach ($frame in $frames) { $writer.Write([byte[]]$frame) }
        $writer.Flush()
        [System.IO.File]::WriteAllBytes((Join-Path $Destination 'lapiswitch.ico'), $iconStream.ToArray())
    } finally {
        $writer.Dispose()
        $iconStream.Dispose()
    }
} finally {
    $sourceBitmap.Dispose()
}
