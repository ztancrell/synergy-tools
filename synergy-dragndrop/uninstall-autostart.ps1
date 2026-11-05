# Undo install-autostart.ps1: remove the "synergy-dragndrop" scheduled task, the Send To entry and
# the firewall rule (one UAC prompt, only if the rule exists), and stop the running app.
# Leaves the program folder, your pairing config and the Downloads folder in place.
# Run: powershell -ExecutionPolicy Bypass -File .\uninstall-autostart.ps1
# ASCII only: Windows PowerShell 5.1 reads BOM-less scripts as ANSI.
$TaskName = "synergy-dragndrop"
$RuleName = "synergy-dragndrop (TCP 24851)"
$dir = Join-Path $env:LOCALAPPDATA "synergy-dragndrop"
$lnk = Join-Path $env:APPDATA "Microsoft\Windows\SendTo\Send to other PC.lnk"

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

$procs = Get-Process -Name "synergy-dragndrop" -ErrorAction SilentlyContinue
if ($procs) {
    $procs | Stop-Process -Force
    Write-Host "Stopped running synergy-dragndrop.exe."
}

if (Test-Path -LiteralPath $lnk) {
    Remove-Item -LiteralPath $lnk -Force
    Write-Host "Removed the Send To entry."
}

if (Get-NetFirewallRule -DisplayName $RuleName -ErrorAction SilentlyContinue) {
    try {
        Start-Process powershell -Verb RunAs -Wait -WindowStyle Hidden `
            -ArgumentList "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "Remove-NetFirewallRule -DisplayName '$RuleName'"
    } catch {
        Write-Host "The administrator prompt was declined or failed: $($_.Exception.Message)"
    }
    if (Get-NetFirewallRule -DisplayName $RuleName -ErrorAction SilentlyContinue) {
        Write-Host "WARNING: firewall rule '$RuleName' is still there. Remove it as administrator:"
        Write-Host "  Remove-NetFirewallRule -DisplayName '$RuleName'"
    } else {
        Write-Host "Removed firewall rule '$RuleName'."
    }
}

Write-Host "Left the folder $dir (program), your pairing config and Downloads in place. To delete the program:"
Write-Host "  Remove-Item -Recurse -Force `"$dir`""
