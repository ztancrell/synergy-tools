# Dev convenience: copy the latest Windows build from the Linux desktop, then start the sender.
# Run: powershell -ExecutionPolicy Bypass -File .\update.ps1 [-Desktop host] [-User name]
param(
    [string]$Desktop = "192.168.254.90",
    [string]$User = "zpt"
)

$exe = Join-Path $PSScriptRoot "synergy-audio.exe"
scp "${User}@${Desktop}:projects/synergy-audio/target/x86_64-pc-windows-gnu/release/synergy-audio.exe" "$exe"
if ($LASTEXITCODE -ne 0) {
    Write-Host "scp failed (exit $LASTEXITCODE). If synergy-audio.exe is still running, close that sender window and retry."
    exit 1
}
& "$exe" send $Desktop
