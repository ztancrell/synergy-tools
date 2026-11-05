# synergy-dragndrop

[![CI](https://github.com/ztancrell/synergy-dragndrop/actions/workflows/ci.yml/badge.svg)](https://github.com/ztancrell/synergy-dragndrop/actions/workflows/ci.yml)

Sends files between a Windows PC and a Linux PC on the same LAN, for machines that share
keyboard and mouse with Synergy 3 (which cannot drag files across). Both machines run the
same program: each one can send files to, and receive files from, the one paired partner.
Files land in the Downloads folder; transfers are authenticated and encrypted after a one-time
pairing with an 8-digit code.

**Status: in development.** Pairing, sending, the drop window and the tray app work (below);
the Windows installer is below, the Linux installer is not written yet.

## Install on Windows

1. Copy `synergy-dragndrop.exe`, `install-autostart.ps1` and `uninstall-autostart.ps1` into one folder.
2. In that folder run `powershell -ExecutionPolicy Bypass -File .\install-autostart.ps1`
   (add `-Peer <other PC's address>` to set the paired PC's address; pairing itself is step 1 of
   "Pairing" below). Rerun it any time to update.
3. Windows shows **one** UAC prompt: it adds the inbound firewall rule for TCP 24851 (Private and
   Public profiles). If you decline, the rest is installed and the script says how to add the rule later.

It installs: the exe in `%LOCALAPPDATA%\synergy-dragndrop\`; a scheduled task `synergy-dragndrop`
that starts the tray app 10 s after every login (no console window); the Explorer entry
**Send to > Send to other PC** (right-click files > Send to); and the firewall rule
`synergy-dragndrop (TCP 24851)`. Undo with `.\uninstall-autostart.ps1` (one more UAC prompt); it keeps
your pairing config and the Downloads folder.

The exe has no console of its own. In a terminal, command-line output still appears in cmd.exe and
PowerShell when you run it from there, but PowerShell may return its prompt before the command has
finished and `$LASTEXITCODE` is unreliable; use `.\synergy-dragndrop.exe pair ... | Out-Host`, or
`Start-Process -Wait -NoNewWindow .\synergy-dragndrop.exe -ArgumentList ...`, or cmd's
`start /wait`. Without a terminal (Send To) results appear as notifications.

## Pairing

Pair once; the pairing is remembered on both PCs. The two PCs must be able to reach each other on
TCP port **24851**, so allow that port in the firewall of the PC that receives (the Windows installer
does this; the Linux installer comes later, until then run `sudo ufw allow 24851/tcp`).

1. On PC 1 run `synergy-dragndrop pair`. It prints a code like `1234-5678` and the command to
   run on the other PC. The code works for one attempt and expires after 2 minutes.
2. On PC 2 run `synergy-dragndrop pair <PC 1's address> 1234-5678`.

A wrong code fails on both sides and the code is spent: start again with a new one.
If a PC's address changes, run `synergy-dragndrop peer <new address>` on the other PC (the
secret is kept); `synergy-dragndrop unpair` forgets the pairing.

## Drop window

Run `synergy-dragndrop` with no arguments: a small window titled `Drop files to send` opens at the
bottom-right of the screen (requested 160x160; the window manager may make it larger, e.g. 200x200 on Openbox). Drag files onto it (from Explorer, Thunar, ...) and they go to the
paired PC; the title shows `Release to send`, `Sending 2 files...`, then `Sent` or `Not sent`, and
a notification gives the result. Closing the window only hides it (tray menu: Show drop window; Quit ends the app). On Windows and on Linux/X11 (e.g. Openbox) the window stays on top by itself. Linux
needs the GTK 3 runtime libraries (`libgtk-3-0`) and `libayatana-appindicator3-1` for the tray icon.

Tip for KDE/GNOME Wayland, where apps can't force "always on top": right-click the title bar, More
Actions, Keep Above Others (or a Window Rule matching the title `Drop files to send`).

## Tray icon

The app lives in the system tray on both PCs (status only: it is not a drop target). Colours:
grey = not paired, green = paired and idle, blue = sending or receiving, red for 5 seconds after an
error. The first menu line (greyed out) shows the status, `Paired with <host>` or `Not paired`
(the hover tooltip with the same text works on Windows only; Linux trays don't get it). Menu: `Hide drop window` / `Show drop
window`, `Pair…` (shows `Pairing code 1234-5678` in the window title for 2 minutes and in a
notification with the command to run on the other PC), `Quit`. If the tray icon can't be created
(library missing), a notification says so and closing the drop window quits the app. If the
receiver thread dies, the app says so and exits. Starting a second copy fails with a
notification, since the first one holds port 24851. On Linux the icon uses libappindicator
(`libayatana-appindicator3`, falls back to an XEmbed icon for panels like tint2 without
StatusNotifierItem support); the Linux build and the CI job need `libayatana-appindicator3-dev`.

## Command line

```
synergy-dragndrop pair               show a code and wait for the other PC to pair
synergy-dragndrop pair <host> <code> pair with the PC showing <code>
synergy-dragndrop peer <host>        change the paired PC's address
synergy-dragndrop unpair             forget the paired PC
synergy-dragndrop send <file>...     send files to the paired PC
synergy-dragndrop listen             receive files (saved to Downloads) until stopped
```

`<host>` is an IP address or name, optionally `:port` (default 24851). `send` takes files only
(folders are refused: zip them first) and fails at once, within 3 seconds, if the other PC is
unreachable; nothing is queued or retried. If the tray app is already running it holds port
24851, so `pair` and `listen` say so; use its tray menu or quit it first.

## Notifications

Results show as desktop notifications, since "Send to" has no terminal: `send` reports success or the error, and a received transfer shows "N files received from <other PC>". Clicking that notification opens the Downloads folder (`xdg-open` on Linux, `explorer.exe` on Windows). If a notification can't be shown, the reason goes to stderr and the transfer is unaffected.

## Where files go

Received files are saved in your Downloads folder.

- A file is written under a hidden temporary name (`.name.xxxxxxxx.part`) and only gets its real
  name once it has arrived completely, so you never see half a file.
- An existing file is never replaced: a second `report.pdf` is saved as `report (1).pdf`,
  then `report (2).pdf`, and so on.
- File names from the other PC are cleaned up: folders in the name are dropped, and characters
  Windows does not allow (`< > : " | ? *`) become `_`.
- On Windows, received files are marked as downloaded from the internet (Mark of the Web,
  `ZoneId=3`), so SmartScreen may ask before you open an executable. On Linux, received files
  are never executable.

## Build from source

```sh
cargo build --release
cargo test
```

The binary is `target/release/synergy-dragndrop`. Run it with a wrong argument for usage (no arguments starts the tray app).
Windows build check from Linux: `cargo check --target x86_64-pc-windows-gnu`.

## License

GPL-3.0-or-later, see [LICENSE](LICENSE).
