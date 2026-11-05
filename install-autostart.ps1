# Start the sender automatically at every Windows login, hidden, logging to a file.
# Registers a Task Scheduler task "synergy-audio" for the current user (runs only while
# you are logged on, in your session, so it can capture your audio). Rerun to reinstall.
# If this PC isn't paired with the receiver yet, it asks for the code that
# `synergy-audio pair` shows on the receiver and pairs first.
# Run (in the folder that holds synergy-audio.exe):
#   powershell -ExecutionPolicy Bypass -File .\install-autostart.ps1 [-Desktop host]
# ASCII only: Windows PowerShell 5.1 reads BOM-less scripts as ANSI.
param(
    [string]$Desktop = "192.168.254.90"
)
$ErrorActionPreference = "Stop"

$TaskName = "synergy-audio"
$src = Join-Path $PSScriptRoot "synergy-audio.exe"
$dir = Join-Path $env:LOCALAPPDATA "synergy-audio"
$exe = Join-Path $dir "synergy-audio.exe"
$log = Join-Path $dir "synergy-audio.log"

if (-not (Test-Path -LiteralPath $src)) {
    Write-Host "synergy-audio.exe not found next to this script ($src)."
    Write-Host "Copy it into this folder first (see README), then rerun."
    exit 1
}

# Stop an older install or a manually started sender, so the exe can be replaced.
Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
Get-Process -Name "synergy-audio" -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

New-Item -ItemType Directory -Force -Path $dir | Out-Null
Copy-Item -LiteralPath $src -Destination $exe -Force
Unblock-File -LiteralPath $exe  # drop the downloaded-from-internet mark a release zip leaves

# The sender won't start without a pairing: pair now if there is none.
if (-not (Test-Path -LiteralPath (Join-Path $env:APPDATA "synergy-audio\receiver"))) {
    Write-Host "Not paired with the receiver yet. On the Linux PC run 'synergy-audio pair'"
    Write-Host "(install-autostart.sh there does it when no sender is paired) and type the code it shows."
    $code = Read-Host "Pairing code"
    & $exe pair $Desktop $code
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Pairing failed; rerun this script to try again."
        exit 1
    }
}

$user = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
# The exe detaches from its console when given --log, so no window stays open.
$action = New-ScheduledTaskAction -Execute "`"$exe`"" -Argument "send $Desktop --log `"$log`"" -WorkingDirectory $dir
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $user
$trigger.Delay = "PT10S"  # give audio and Wi-Fi a moment after login
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
    -ExecutionTimeLimit ([TimeSpan]::Zero) `
    -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) `
    -MultipleInstances IgnoreNew

try {
    Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger `
        -Principal $principal -Settings $settings -Force `
        -Description "Stream this PC's audio to $Desktop (synergy-audio)" | Out-Null
} catch {
    Write-Host "Registering the scheduled task failed: $($_.Exception.Message)"
    if ("$($_.Exception.Message)" -match "denied|0x80070005") {
        Write-Host "Hint: open PowerShell as administrator (right-click > Run as administrator),"
        Write-Host "cd to this folder and rerun this script."
    }
    exit 1
}

Start-ScheduledTask -TaskName $TaskName
Start-Sleep -Seconds 3

Write-Host ""
Write-Host "Installed: task '$TaskName' starts the sender at every login of $user (after 10 s)."
Write-Host "  Program: $exe"
Write-Host "  Sends to: $Desktop"
Write-Host "  Log:     $log"
if (Get-Process -Name "synergy-audio" -ErrorAction SilentlyContinue) {
    Write-Host "It is running now (Task Manager > Details > synergy-audio.exe)."
} else {
    Write-Host "WARNING: synergy-audio.exe is not running. Check the log below and Task Scheduler"
    Write-Host "(taskschd.msc > Task Scheduler Library > $TaskName > History / Last Run Result)."
}
if (Test-Path -LiteralPath $log) {
    Write-Host "Last log lines:"
    Get-Content -LiteralPath $log -Tail 5
}
