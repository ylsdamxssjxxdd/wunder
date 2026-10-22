# Generate a Win7-compatible multi-size ICO (32-bit DIB entries) from a PNG.
#
# Pure PowerShell / System.Drawing: no Python, Pillow, OpenCV or ImageMagick
# needed. The DIB layout mirrors scripts/generate_servo_icon.py so the old MinGW
# windres keeps accepting the result (PNG-compressed ICO entries are rejected).
#
# The source alpha is used as-is: the visible content is cropped to its alpha
# bounding box, padded to a square and scaled to the canvas before the per-size
# frames are produced.
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\generate_windows_icon.ps1 `
#     -Source images\asset-154424-transparent.png -Target images\asset-154424-transparent.ico

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Target,
    [int[]]$Sizes = @(16, 24, 32, 48, 64, 128, 256),
    [int]$Canvas = 256
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

function New-ArgbBitmap([int]$width, [int]$height) {
    return New-Object System.Drawing.Bitmap -ArgumentList @(
        $width, $height, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
}

function New-HqGraphics([System.Drawing.Image]$image) {
    $graphics = [System.Drawing.Graphics]::FromImage($image)
    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    return $graphics
}

function New-Rect([int]$x, [int]$y, [int]$width, [int]$height) {
    return New-Object System.Drawing.Rectangle -ArgumentList @($x, $y, $width, $height)
}

function Get-ArgbPixels([System.Drawing.Bitmap]$bitmap) {
    $rect = New-Rect 0 0 $bitmap.Width $bitmap.Height
    $data = $bitmap.LockBits(
        $rect,
        [System.Drawing.Imaging.ImageLockMode]::ReadOnly,
        [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
        $bytes = New-Object byte[] ($data.Stride * $bitmap.Height)
        [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $bytes, 0, $bytes.Length)
        return [pscustomobject]@{
            Bytes  = $bytes
            Stride = $data.Stride
            Width  = $bitmap.Width
            Height = $bitmap.Height
        }
    }
    finally {
        $bitmap.UnlockBits($data)
    }
}

function Get-AlphaBounds([pscustomobject]$pixels) {
    $minX = [int]::MaxValue; $minY = [int]::MaxValue; $maxX = -1; $maxY = -1
    for ($y = 0; $y -lt $pixels.Height; $y++) {
        $row = $y * $pixels.Stride
        for ($x = 0; $x -lt $pixels.Width; $x++) {
            if ($pixels.Bytes[$row + $x * 4 + 3] -ne 0) {
                if ($x -lt $minX) { $minX = $x }
                if ($x -gt $maxX) { $maxX = $x }
                if ($y -lt $minY) { $minY = $y }
                if ($y -gt $maxY) { $maxY = $y }
            }
        }
    }
    if ($maxX -lt 0) { return $null }
    return [pscustomobject]@{
        X      = $minX
        Y      = $minY
        Width  = ($maxX - $minX + 1)
        Height = ($maxY - $minY + 1)
    }
}

# One BITMAPINFOHEADER + bottom-up 32-bit BGRA pixels + empty AND mask.
function New-DibEntry([System.Drawing.Bitmap]$bitmap) {
    $size = [int]$bitmap.Width
    $pixels = Get-ArgbPixels $bitmap
    $xorLength = $size * $size * 4
    $andStride = [int](([math]::Floor(($size + 31) / 32)) * 4)
    $andLength = $andStride * $size
    $total = 40 + $xorLength + $andLength
    $payload = New-Object byte[] $total

    $header = New-Object byte[] 40
    [System.BitConverter]::GetBytes([int]40).CopyTo($header, 0)
    [System.BitConverter]::GetBytes([int]$size).CopyTo($header, 4)
    [System.BitConverter]::GetBytes([int]($size * 2)).CopyTo($header, 8)
    [System.BitConverter]::GetBytes([int16]1).CopyTo($header, 12)
    [System.BitConverter]::GetBytes([int16]32).CopyTo($header, 14)
    [System.BitConverter]::GetBytes([int]0).CopyTo($header, 16)
    [System.BitConverter]::GetBytes([int]($xorLength + $andLength)).CopyTo($header, 20)
    [System.BitConverter]::GetBytes([int]0).CopyTo($header, 24)
    [System.BitConverter]::GetBytes([int]0).CopyTo($header, 28)
    [System.BitConverter]::GetBytes([int]0).CopyTo($header, 32)
    [System.BitConverter]::GetBytes([int]0).CopyTo($header, 36)
    [System.Array]::Copy($header, 0, $payload, 0, 40)

    $position = 40
    for ($y = $size - 1; $y -ge 0; $y--) {
        [System.Array]::Copy($pixels.Bytes, ($y * $pixels.Stride), $payload, $position, ($size * 4))
        $position += $size * 4
    }
    return , $payload
}

$sourcePath = (Resolve-Path -LiteralPath $Source).Path
$targetPath = [System.IO.Path]::GetFullPath($Target)

$loaded = New-Object System.Drawing.Bitmap -ArgumentList @($sourcePath)
$work = New-ArgbBitmap $loaded.Width $loaded.Height
$graphics = New-HqGraphics $work
$graphics.DrawImage($loaded, (New-Rect 0 0 $loaded.Width $loaded.Height))
$graphics.Dispose()
$loaded.Dispose()

$bounds = Get-AlphaBounds (Get-ArgbPixels $work)
if ($bounds) {
    $srcRect = New-Rect $bounds.X $bounds.Y $bounds.Width $bounds.Height
} else {
    $srcRect = New-Rect 0 0 $work.Width $work.Height
}
$side = [math]::Max($work.Width, $work.Height)
$square = New-ArgbBitmap $side $side
$graphics = New-HqGraphics $square
$graphics.DrawImage(
    $work,
    (New-Rect ([int](($side - $srcRect.Width) / 2)) ([int](($side - $srcRect.Height) / 2)) $srcRect.Width $srcRect.Height),
    $srcRect,
    [System.Drawing.GraphicsUnit]::Pixel)
$graphics.Dispose()
$work.Dispose()

$canvasImage = New-ArgbBitmap $Canvas $Canvas
$graphics = New-HqGraphics $canvasImage
$graphics.DrawImage($square, (New-Rect 0 0 $Canvas $Canvas))
$graphics.Dispose()
$square.Dispose()

$frames = New-Object 'System.Collections.Generic.List[byte[]]'
foreach ($size in $Sizes) {
    $frame = New-ArgbBitmap $size $size
    $graphics = New-HqGraphics $frame
    $graphics.DrawImage($canvasImage, (New-Rect 0 0 $size $size))
    $graphics.Dispose()
    $payload = New-DibEntry $frame
    if ($null -eq $payload -or $payload.Length -lt 1024) {
        throw "empty DIB payload for ${size}x${size}"
    }
    $frames.Add([byte[]]$payload)
    $frame.Dispose()
}
$canvasImage.Dispose()

$stream = New-Object System.IO.MemoryStream
$writer = New-Object System.IO.BinaryWriter -ArgumentList @($stream)
try {
    $writer.Write([int16]0)
    $writer.Write([int16]1)
    $writer.Write([int16]$Sizes.Count)
    $offset = 6 + 16 * $Sizes.Count
    for ($i = 0; $i -lt $Sizes.Count; $i++) {
        $size = $Sizes[$i]
        $dimension = 0
        if ($size -lt 256) { $dimension = $size }
        $writer.Write([byte]$dimension)
        $writer.Write([byte]$dimension)
        $writer.Write([byte]0)
        $writer.Write([byte]0)
        $writer.Write([int16]1)
        $writer.Write([int16]32)
        $writer.Write([int]$frames[$i].Length)
        $writer.Write([int]$offset)
        $offset += $frames[$i].Length
    }
    foreach ($payload in $frames) { $writer.Write([byte[]]$payload) }
    $writer.Flush()
    [System.IO.File]::WriteAllBytes($targetPath, $stream.ToArray())
}
finally {
    $writer.Dispose()
    $stream.Dispose()
}

if ($bounds) {
    Write-Output ("alpha bounds: {0},{1} {2}x{3}" -f $bounds.X, $bounds.Y, $bounds.Width, $bounds.Height)
} else {
    Write-Output "alpha bounds: fully opaque, using the whole image"
}
$info = Get-Item -LiteralPath $targetPath
Write-Output ("wrote {0} ({1} bytes, {2} frames: {3})" -f $targetPath, $info.Length, $Sizes.Count, ($Sizes -join ","))