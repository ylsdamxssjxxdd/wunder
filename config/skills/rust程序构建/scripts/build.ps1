param([string]$Target='win7-x86',[string]$SdkRoot=$env:RUST_BUILDER_ROOT,[string]$Kind='cli',[string]$Profile='release',[string]$Package,[int]$Jobs=0,[string]$Output='target/dist')
$ErrorActionPreference='Stop'; if (-not $SdkRoot) { $SdkRoot=Join-Path (Split-Path (Get-Location) -Parent) 'Rust-builder' }
python (Join-Path $PSScriptRoot 'preflight.py') --target $Target --sdk-root $SdkRoot
$triple=@{'win7-x86'='i686-win7-windows-gnu';'linux-amd64-ubuntu18'='x86_64-unknown-linux-gnu';'linux-arm64-ubuntu18'='aarch64-unknown-linux-gnu';'kylin-x86'='x86_64-unknown-linux-gnu'}[$Target]
$env:CARGO_NET_OFFLINE='true'; $args=@('build','--locked','--offline','--profile',$Profile,'--target',$triple); if($Package){$args += @('--package',$Package)}; if($Jobs -gt 0){$env:CARGO_BUILD_JOBS="$Jobs"}; cargo @args
New-Item -ItemType Directory -Force $Output | Out-Null; Write-Host "Built $Kind for $Target ($triple). Copy the selected binary from target/$triple/$Profile to $Output and run package.py."
