@echo off
setlocal
cd /d "%~dp0"
echo [AlKAsH3D] OpenXR capability probe
echo Start SteamVR (or your headset vendor app) and connect the headset first.
cargo run --release --features xr --bin xr_probe
echo.
pause
