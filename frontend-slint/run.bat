@echo off
setlocal
cargo +1.95.0-x86_64-pc-windows-msvc run --release --locked -j 8 --manifest-path "%~dp0Cargo.toml" --target-dir "%~dp0..\target\frontend-slint"
exit /b %errorlevel%
