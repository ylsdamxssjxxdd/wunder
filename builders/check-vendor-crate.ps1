# Verifies that every file hash recorded in a vendored crate's
# .cargo-checksum.json matches the file on disk, so a partially copied crate
# cannot silently break an offline build.
param(
    [Parameter(Mandatory = $true)][string]$VendorRoot,
    [string[]]$Crate = @()
)

$ErrorActionPreference = "Stop"
$VendorRoot = [IO.Path]::GetFullPath($VendorRoot)
if (!(Test-Path -LiteralPath $VendorRoot)) { throw "Vendor root not found: $VendorRoot" }

$crates = if ($Crate.Count -gt 0) {
    $Crate | ForEach-Object { Join-Path $VendorRoot $_ }
} else {
    Get-ChildItem -LiteralPath $VendorRoot -Directory | ForEach-Object { $_.FullName }
}

$failed = 0
foreach ($crateDir in $crates) {
    if (!(Test-Path -LiteralPath $crateDir)) { throw "Crate directory not found: $crateDir" }
    $checksumFile = Join-Path $crateDir ".cargo-checksum.json"
    if (!(Test-Path -LiteralPath $checksumFile)) {
        Write-Output "FAIL  $(Split-Path -Leaf $crateDir): .cargo-checksum.json missing"
        $failed++
        continue
    }
    $manifest = Get-Content -LiteralPath $checksumFile -Raw | ConvertFrom-Json
    $mismatch = @()
    $missing = @()
    foreach ($entry in $manifest.files.PSObject.Properties) {
        $relative = $entry.Name
        $path = Join-Path $crateDir ($relative -replace '/', [IO.Path]::DirectorySeparatorChar)
        if (!(Test-Path -LiteralPath $path -PathType Leaf)) { $missing += $relative; continue }
        $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $entry.Value) { $mismatch += $relative }
    }
    if ($missing.Count -gt 0 -or $mismatch.Count -gt 0) {
        $failed++
        Write-Output ("FAIL  {0}: missing={1} mismatch={2}" -f (Split-Path -Leaf $crateDir), $missing.Count, $mismatch.Count)
        ($missing + $mismatch) | Select-Object -First 5 | ForEach-Object { Write-Output "        $_" }
    } else {
        Write-Output ("OK    {0} ({1} files)" -f (Split-Path -Leaf $crateDir), $manifest.files.PSObject.Properties.Count)
    }
}

if ($failed -gt 0) { throw "$failed vendored crate(s) failed checksum verification" }
Write-Output "All checked crates verified."
