# Undo install-autostart.ps1: remove the "synergy-audio" scheduled task and stop the sender.
# Then unmutes the speakers if the sender had auto-muted them.
# Leaves the program folder (exe + log) in place; the output says how to delete it.
# Run: powershell -ExecutionPolicy Bypass -File .\uninstall-autostart.ps1
# ASCII only: Windows PowerShell 5.1 reads BOM-less scripts as ANSI.
$TaskName = "synergy-audio"
$dir = Join-Path $env:LOCALAPPDATA "synergy-audio"

if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) {
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    try {
        Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction Stop
        Write-Host "Removed scheduled task '$TaskName': it no longer starts at login."
    } catch {
        Write-Host "Removing the task failed: $($_.Exception.Message)"
        Write-Host "Hint: open PowerShell as administrator and rerun this script."
        exit 1
    }
} else {
    Write-Host "No scheduled task '$TaskName' found (already uninstalled)."
}

$procs = Get-Process -Name "synergy-audio" -ErrorAction SilentlyContinue
if ($procs) {
    $procs | Stop-Process -Force
    Write-Host "Stopped running synergy-audio.exe."
}
# A killed sender can't unmute the speakers itself; undo its auto-mute (no-op if it hadn't muted).
$exe = Join-Path $dir "synergy-audio.exe"
if (Test-Path -LiteralPath $exe) {
    & $exe unmute
}

Write-Host "Left the folder $dir (program and log) in place. To delete it:"
Write-Host "  Remove-Item -Recurse -Force `"$dir`""
