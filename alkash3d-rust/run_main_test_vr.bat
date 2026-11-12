@echo off
setlocal
cd /d "%~dp0"
echo [AlKAsH3D] Starting main_test in VR
echo Start SteamVR and connect the headset before continuing.
cargo run --release --features xr --bin main_test -- --vr
echo.
pause
