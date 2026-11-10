@echo off
setlocal
cd /d "%~dp0"
echo [AlKAsH3D] Starting stereo VR test scene
echo SteamVR and the headset must already be ready.
cargo run --release --features xr --bin xr_demo
echo.
pause
