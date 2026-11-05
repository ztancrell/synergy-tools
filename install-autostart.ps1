# Install synergy-dragndrop on Windows: tray app at every login, "Send to other PC" in Explorer's
# Send To menu, and an inbound firewall rule for TCP 24851 (one UAC prompt).
# Registers a Task Scheduler task "synergy-dragndrop" for the current user (runs only while you
# are logged on, in your session). Rerun to reinstall or update.
# Run (in the folder that holds synergy-dragndrop.exe):
#   powershell -ExecutionPolicy Bypass -File .\install-autostart.ps1 [-Peer host]
# -Peer sets the paired PC's address (needs an existing pairing; see README).
# ASCII only: Windows PowerShell 5.1 reads BOM-less scripts as ANSI.
param(
    [string]$Peer = ""
)
$ErrorActionPreference = "Stop"

$TaskName = "synergy-dragndrop"
$RuleName = "synergy-dragndrop (TCP 24851)"
$src = Join-Path $PSScriptRoot "synergy-dragndrop.exe"
$dir = Join-Path $env:LOCALAPPDATA "synergy-dragndrop"
$exe = Join-Path $dir "synergy-dragndrop.exe"
$exeQ = $exe.Replace("'", "''")  # for single-quoted strings
$lnk = Join-Path $env:APPDATA "Microsoft\Windows\SendTo\Send to other PC.lnk"

if (-not (Test-Path -LiteralPath $src)) {
    Write-Host "synergy-dragndrop.exe not found next to this script ($src)."
    Write-Host "Copy it into this folder first (see README), then rerun."
    exit 1
}

# Stop an older install or a manually started copy, so the exe can be replaced.
Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
Get-Process -Name "synergy-dragndrop" -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

New-Item -ItemType Directory -Force -Path $dir | Out-Null
# Rerun from the install folder: source is the target already.
if ((Resolve-Path -LiteralPath $src).Path -ne $exe) {
    Copy-Item -LiteralPath $src -Destination $exe -Force
}
Unblock-File -LiteralPath $exe  # drop the downloaded-from-internet mark a release zip leaves

if ($Peer) {
    # The exe is a GUI-subsystem program: Start-Process -Wait gives a reliable exit code.
    $p = Start-Process -FilePath $exe -ArgumentList "peer", $Peer -Wait -PassThru -NoNewWindow
    if ($p.ExitCode -ne 0) {
        Write-Host "Setting the peer address failed (not paired yet?). Pair first, see README."
        exit 1
    }
}

$user = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
# No arguments = tray app; it has no console window.
$action = New-ScheduledTaskAction -Execute "`"$exe`"" -WorkingDirectory $dir
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $user
$trigger.Delay = "PT10S"  # give the desktop and Wi-Fi a moment after login
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
    -ExecutionTimeLimit ([TimeSpan]::Zero) `
    -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) `
    -MultipleInstances IgnoreNew

try {
    Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger `
        -Principal $principal -Settings $settings -Force `
        -Description "synergy-dragndrop tray app: send and receive files with the paired PC" | Out-Null
} catch {
    Write-Host "Registering the scheduled task failed: $($_.Exception.Message)"
    if ("$($_.Exception.Message)" -match "denied|0x80070005") {
        Write-Host "Hint: open PowerShell as administrator (right-click > Run as administrator),"
        Write-Host "cd to this folder and rerun this script."
    }
    exit 1
}

# Explorer appends each selected file to the arguments: "exe send file1 file2 ...".
$sc = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
$sc.TargetPath = $exe
$sc.Arguments = "send"
$sc.WorkingDirectory = $dir
$sc.IconLocation = "$exe,0"
$sc.Save()

# Inbound firewall rule: needs admin, so ask once (UAC) and only if the rule is missing.
$ruleOk = [bool](Get-NetFirewallRule -DisplayName $RuleName -ErrorAction SilentlyContinue)
if (-not $ruleOk) {
    $cmd = "New-NetFirewallRule -DisplayName '$RuleName' -Direction Inbound -Protocol TCP " +
        "-LocalPort 24851 -Program '$exeQ' -Profile Private,Public -Action Allow | Out-Null"
    try {
        Start-Process powershell -Verb RunAs -Wait -WindowStyle Hidden `
            -ArgumentList "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", $cmd
    } catch {
        Write-Host "The administrator prompt was declined or failed: $($_.Exception.Message)"
    }
    $ruleOk = [bool](Get-NetFirewallRule -DisplayName $RuleName -ErrorAction SilentlyContinue)
}

Start-ScheduledTask -TaskName $TaskName
Start-Sleep -Seconds 3

Write-Host ""
Write-Host "Installed: task '$TaskName' starts the tray app at every login of $user (after 10 s)."
Write-Host "  Program:  $exe"
Write-Host "  Send To:  $lnk"
if ($ruleOk) {
    Write-Host "  Firewall: rule '$RuleName' is in place."
} else {
    Write-Host "  WARNING: firewall rule '$RuleName' is missing, so this PC cannot receive files."
    Write-Host "  Rerun this script and accept the administrator prompt, or run as administrator:"
    Write-Host "    New-NetFirewallRule -DisplayName '$RuleName' -Direction Inbound -Protocol TCP -LocalPort 24851 -Program '$exeQ' -Profile Private,Public -Action Allow"
}
if (Get-Process -Name "synergy-dragndrop" -ErrorAction SilentlyContinue) {
    Write-Host "It is running now (tray icon; Task Manager > Details > synergy-dragndrop.exe)."
} else {
    Write-Host "WARNING: synergy-dragndrop.exe is not running. Check Task Scheduler"
    Write-Host "(taskschd.msc > Task Scheduler Library > $TaskName > History / Last Run Result)."
}
if (-not (Test-Path -LiteralPath (Join-Path $env:APPDATA "synergy-dragndrop"))) {
    Write-Host "Not paired yet: see 'Pairing' in the README."
}
