@echo off
setlocal
cd /d "%~dp0"
echo [AlKAsH3D] Starting Buckshot VR scene
echo Start SteamVR and connect the headset before continuing.
cargo run --release --features xr --bin buckshot -- --vr
echo.
pause
