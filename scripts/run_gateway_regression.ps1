param(
    [switch]$NoCapture
)

$args = @("test", "--test", "gateway_regression")
if (-not $NoCapture) {
    $args += @("--", "--nocapture")
}

Write-Host "Running gateway regression suite..." -ForegroundColor Cyan
cargo @args
