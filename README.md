# synergy-dragndrop

[![CI](https://github.com/ztancrell/synergy-dragndrop/actions/workflows/ci.yml/badge.svg)](https://github.com/ztancrell/synergy-dragndrop/actions/workflows/ci.yml)

Sends files between a Windows PC and a Linux PC on the same LAN, for machines that share
keyboard and mouse with Synergy 3 (which cannot drag files across). Both machines run the
same program: each one can send files to, and receive files from, the one paired partner.
Files land in the Downloads folder; transfers are authenticated and encrypted after a one-time
pairing with an 8-digit code.

**Status:** v0.1.0. Windows <-> Linux, both directions. Not tagged or released yet (no GitHub
remote yet), so build from source for now (see "Build from source" for the Windows exe); the
release steps below describe what a tagged release will contain.

The Linux target is Openbox + tint2 on X11 with Thunar and dunst. KDE and GNOME work as extras.

## Download and install

A release has two files plus `SHA256SUMS`: `synergy-dragndrop-vX.Y.Z-linux-x86_64.tar.gz` and
`synergy-dragndrop-vX.Y.Z-windows-x86_64.zip`. Each holds the program, the install and uninstall
scripts for that OS, this README and the LICENSE.

### Linux

Extract the tarball and run `./install-autostart.sh` inside the folder (from a source checkout it
builds with cargo instead; from an extracted release it uses the bundled binary). It installs:

- `~/.local/bin/synergy-dragndrop`, started now and at every login. Autostart is one mechanism
  only: the XDG entry `~/.config/autostart/synergy-dragndrop.desktop` (Openbox runs it through
  `openbox-xdg-autostart`); only if `~/.config/openbox/autostart` exists and no
  `openbox-xdg-autostart` is found, it appends the line `<bin> & # synergy-dragndrop` there instead.
- Right-click "Send to other PC": Thunar (`~/.local/share/Thunar/sendto`, shown as Send To for
  files), plus Dolphin (`kio/servicemenus`) and Nautilus (`nautilus/scripts`).
- A firewall rule for TCP 24851 in ufw or firewalld, if one is active. Only this step asks for
  your sudo password.

The program needs the GTK 3 runtime and the Ayatana appindicator library for the tray icon:
Debian/Ubuntu `libgtk-3-0` and `libayatana-appindicator3-1`; Arch/CachyOS `gtk3` and
`libayatana-appindicator`.

### Windows

1. Get one folder holding `synergy-dragndrop.exe`, `install-autostart.ps1`,
   `uninstall-autostart.ps1` and the README: extract the release zip (Explorer: Extract All;
   everything is at the zip root), or, until there are releases, build the exe on the Linux PC and
   copy those files over (see "Build from source").
2. In that folder run `powershell -ExecutionPolicy Bypass -File .\install-autostart.ps1`
   (add `-Peer <other PC's address>` to set the paired PC's address once you are paired).
   Rerun it any time to update.
3. Windows shows **one** UAC prompt: it adds the inbound firewall rule for TCP 24851 (Private and
   Public profiles). If you decline, the rest is installed and the script says how to add the rule later.

It installs: the exe in `%LOCALAPPDATA%\synergy-dragndrop\`; a scheduled task `synergy-dragndrop`
that starts the tray app 10 s after every login (no console window); the Explorer entry
**Send to > Send to other PC** (right-click files > Send to); and the firewall rule
`synergy-dragndrop (TCP 24851)`.

The exe has no console of its own, so in a terminal PowerShell may return its prompt before the
command has finished, and `$LASTEXITCODE` is unreliable. Every example below therefore ends in
`| Out-Host`; the alternative is
`Start-Process -Wait -NoNewWindow .\synergy-dragndrop.exe -ArgumentList ...` (or cmd's
`start /wait`). Without a terminal (Send To) results appear as notifications.

### Pairing

Pair once; the pairing is remembered on both PCs. The two PCs must reach each other on TCP port
**24851** (the installers open it).

1. On PC 1 either use the tray menu **Pair…** (the code shows in the drop window's title and in a
   notification, with the command for the other PC) or run `synergy-dragndrop pair`. It prints a
   code like `1234-5678` and the command to run on the other PC. The code works for one attempt
   and expires after 2 minutes.
2. On PC 2 run `synergy-dragndrop pair <PC 1's address> 1234-5678`
   (Windows: `.\synergy-dragndrop.exe pair <address> 1234-5678 | Out-Host`).

Both sides print or show `Paired with ...` and the tray icons turn green. A wrong code fails on both
sides and the code is spent: start again with a new one. If a PC's address changes, run
`synergy-dragndrop peer <new address>` on the other PC (the secret is kept); `synergy-dragndrop
unpair` forgets the pairing. The pairing is the file `peer` in `~/.config/synergy-dragndrop/`
(Linux, mode 0600) or `%APPDATA%\synergy-dragndrop\` (Windows).

## Daily use

- **Drop window:** a small window titled `Drop files to send` sits at the bottom-right of the
  screen (requested 160x160; the window manager may make it larger, e.g. 200x200 on Openbox).
  Drag files onto it (from Explorer, Thunar, ...). The title shows `Release to send`,
  `Sending 2 files…`, then `Sent ✓` or `Not sent`, and a notification gives the result. Closing the
  window only hides it (tray menu: Show drop window). On Windows and on Linux/X11 (e.g. Openbox)
  the window stays on top by itself.
- **Right-click:** Windows Explorer: right-click > Send to > Send to other PC. Thunar: right-click >
  Send To > Send to other PC. Dolphin and Nautilus have an entry too.
- **Tray icon** (status only, not a drop target). Colours: grey = not paired, green = paired and
  idle, blue = sending or receiving, red for 5 seconds after an error. The first menu line (greyed
  out) shows the status, `Paired with <host>` or `Not paired` (the hover tooltip with the same text
  works on Windows only; Linux trays don't get it). Menu: `Hide drop window` / `Show drop window`,
  `Pair…`, `Quit`. If the tray icon can't be created (library missing), a notification says so
  and closing the drop window quits the app. If the receiver thread dies, the app says so and
  exits. Starting a second copy fails with a notification, since the first one holds port 24851.
  On Linux the icon uses libappindicator (`libayatana-appindicator3`, with a fallback to an XEmbed
  icon for panels like tint2 without StatusNotifierItem support).

### Command line

```
synergy-dragndrop                    tray app (drop window, receives files)
synergy-dragndrop pair               show a code and wait for the other PC to pair
synergy-dragndrop pair <host> <code> pair with the PC showing <code>
synergy-dragndrop peer <host>        change the paired PC's address
synergy-dragndrop unpair             forget the paired PC
synergy-dragndrop send <file>...     send files to the paired PC
synergy-dragndrop listen             receive files without the tray app
synergy-dragndrop --version
```

On Windows use `.\synergy-dragndrop.exe ... | Out-Host`. `<host>` is an IP address or name,
optionally `:port` (default 24851). `send` takes files only (folders are refused: zip them first)
and fails at once, within 3 seconds, if the other PC is unreachable; nothing is queued or retried.
If the tray app is already running it holds port 24851, so `pair` and `listen` say so; use its tray
menu or quit it first.

## Where files go

Received files are saved in your Downloads folder.

- A file is written under a temporary name (`.name.xxxxxxxx.part`, hidden on Linux, visible on
  Windows) and only gets its real name once it has arrived completely, so you never see half a
  file. A crash mid-receive can leave one behind; it is safe to delete and never looks like a
  complete file.
- An existing file is never replaced: a second `report.pdf` is saved as `report (1).pdf`,
  then `report (2).pdf`, and so on.
- File names from the other PC are cleaned up: folders in the name are dropped, and characters
  Windows does not allow (`< > : " | ? *`) become `_`.
- On Windows, received files are marked as downloaded from the internet (Mark of the Web,
  `ZoneId=3`), so SmartScreen may ask before you open an executable. On Linux, received files
  are never executable.

## Notifications

Results show as desktop notifications, since "Send to" has no terminal: `send` reports success or
the error, and a received transfer shows `N files received from <other PC>` (`1 file received from
...` for one). Opening the Downloads folder (`xdg-open` on Linux, `explorer.exe` on Windows):

- Windows, KDE, GNOME: click the notification.
- dunst (Openbox): left-click only closes the notification; **middle-click opens Downloads**
  (dunst's default `mouse_middle_click = do_action`).

If a notification can't be shown, the reason goes to stderr and the transfer is unaffected.

## Troubleshooting

- **`not sent: other PC unreachable`** (after 3 seconds): the other PC is off, the address in the
  pairing is out of date (`synergy-dragndrop peer <new address>`; a fixed address for it in your
  router avoids this), or its firewall blocks TCP 24851. Rerun the installer there, or on Windows
  `New-NetFirewallRule -DisplayName 'synergy-dragndrop (TCP 24851)' -Direction Inbound -Protocol TCP -LocalPort 24851 -Program "$env:LOCALAPPDATA\synergy-dragndrop\synergy-dragndrop.exe" -Profile Private,Public -Action Allow`
  as administrator. The rule covers only that installed exe: a `listen` or `pair` run from another
  folder is not covered by it.
- **`not sent: <name> is a folder, zip it first`**: only files are sent.
- **`not sent: the other PC refused (pairing doesn't match: pair again)`**: unpair and pair again.
- **`port 24851 is busy: synergy-dragndrop is probably already running`**: the tray app holds the
  port. Use its tray menu **Pair…** or quit it before running `pair` or `listen` in a terminal.
- **Windows shows "Windows protected your PC" (SmartScreen)** for a received `.exe`: click **More
  info > Run anyway**. That is the Mark of the Web at work; the program adds no dialog of its own.
- **No terminal output on Windows**: add `| Out-Host` (see above).
- **Clicking a notification does nothing on Openbox/dunst**: use middle-click (see Notifications).
- **No tray icon on Linux**: install `libayatana-appindicator3-1` (Arch/CachyOS: `libayatana-appindicator`); tint2 needs its systray enabled.
- Extra, KDE/GNOME Wayland (apps can't force "always on top"): right-click the drop window's title
  bar > More Actions > Keep Above Others.
- A batch where a later file fails (disk full, say) leaves the earlier files saved although the
  sender sees `not sent` for the whole batch.

## Uninstall

- Linux: `./uninstall-autostart.sh` removes the binary, autostart entry and Thunar/Dolphin/Nautilus
  entries. It keeps your pairing config and the firewall rule, and prints the command to close the port.
- Windows: `powershell -ExecutionPolicy Bypass -File .\uninstall-autostart.ps1` removes the task,
  the Send To entry and the firewall rule (one more UAC prompt). It keeps the program folder, your
  pairing config and Downloads; it prints the command to delete the folder.

## Build from source

```sh
cargo build --release
cargo test
```

The binary is `target/release/synergy-dragndrop`. Linux needs `libgtk-3-dev` and
`libayatana-appindicator3-dev` (Arch/CachyOS: `gtk3` and `libayatana-appindicator`).

Windows exe, cross-built on the Linux PC (until releases exist):

```sh
rustup target add x86_64-pc-windows-gnu
# mingw-w64: Arch `mingw-w64-gcc`; Debian/Ubuntu `gcc-mingw-w64-x86-64`
RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-pc-windows-gnu
```

Copy `target/x86_64-pc-windows-gnu/release/synergy-dragndrop.exe`, `install-autostart.ps1`,
`uninstall-autostart.ps1` and `README.md` into one folder on the Windows PC and follow the Windows
install steps above. Alternatively run a native MSVC `cargo build --release` on Windows. A quick
check without linking: `cargo check --target x86_64-pc-windows-gnu`. A tag `vX.Y.Z` (which must equal the version in
`Cargo.toml`) runs the release workflow: both archives and `SHA256SUMS`.

## How it works

TCP port 24851, one connection per batch. The first byte says what it is: `P` pairing, `S` session.
Pairing runs SPAKE2 over the 8-digit code (one attempt, 120 s) and both sides keep a shared secret.
A session is a Noise handshake (`Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s`) with that secret as
pre-shared key, then encrypted messages: batch header (count, sender name), then for each file a
header (size, name), data chunks and an end marker; the receiver answers with the number saved
or a failure reason. Frames are a 16-bit length plus bytes. Connect timeout 3 s, stall timeout 30 s.

## Security notes

- Only the paired PC can send: a connection that fails the handshake is refused and
  `refused connection from <ip>: not the paired PC` is printed by `listen` and `pair` in the
  terminal (the tray app has no log).
- Transfers are encrypted and authenticated, but the 8-digit code is short: pair on a network you
  trust, within the 2-minute window. Anyone who guesses it in the one attempt would be paired, so
  a failed attempt ends pairing mode.
- The secret sits unencrypted in the `peer` file, readable only by you on Linux.
- Received names are sanitised and files are never overwritten or made executable (Linux).
- LAN only; nothing goes through the internet.

## License

GPL-3.0-or-later, see [LICENSE](LICENSE).
