# Dev convenience: copy the latest Windows build from the Linux desktop, then start the sender.
# Run: powershell -ExecutionPolicy Bypass -File .\update.ps1 [-Desktop host] [-User name]
# If autostart is installed (scheduled task "synergy-audio", see install-autostart.ps1), it
# updates that copy and restarts the task instead of running a sender in this window.
# ASCII only: Windows PowerShell 5.1 reads BOM-less scripts as ANSI.
param(
    [string]$Desktop = "192.168.254.90",
    [string]$User = "zpt"
)

$TaskName = "synergy-audio"
$remote = "${User}@${Desktop}:projects/synergy-audio/target/x86_64-pc-windows-gnu/release/synergy-audio.exe"

if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) {
    $dir = Join-Path $env:LOCALAPPDATA "synergy-audio"
    $exe = Join-Path $dir "synergy-audio.exe"
    $log = Join-Path $dir "synergy-audio.log"
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    Get-Process -Name "synergy-audio" -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Seconds 1
    scp "$remote" "$exe"
    $scpExit = $LASTEXITCODE
    Start-ScheduledTask -TaskName $TaskName
    if ($scpExit -ne 0) {
        Write-Host "scp failed (exit $scpExit); restarted the autostart sender on the old build."
        exit 1
    }
    Write-Host "Updated $exe and restarted task '$TaskName'. Log: $log"
    Start-Sleep -Seconds 6
    if (Test-Path -LiteralPath $log) {
        Get-Content -LiteralPath $log -Tail 3
    }
    exit 0
}

$exe = Join-Path $PSScriptRoot "synergy-audio.exe"
scp "$remote" "$exe"
if ($LASTEXITCODE -ne 0) {
    Write-Host "scp failed (exit $LASTEXITCODE). If synergy-audio.exe is still running, close that sender window and retry."
    exit 1
}
& "$exe" send $Desktop
