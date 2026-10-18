@echo off
rem Склеивает пак пони из частей / Joins the pony pack from its parts.
cd /d "%~dp0"
copy /b all_v15c.mlpo.part00 + all_v15c.mlpo.part01 all_v15c.mlpo >nul
echo all_v15c.mlpo ready. SHA-256 must be:
echo 1f7195783148bd67a676edaafa458f872cbc6fb5b7e0eac27fcd9c9234565c31
certutil -hashfile all_v15c.mlpo SHA256
