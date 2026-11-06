# synergy-tools: files

[![CI](https://github.com/ztancrell/synergy-tools/actions/workflows/ci.yml/badge.svg)](https://github.com/ztancrell/synergy-tools/actions/workflows/ci.yml)

Sends files between a Windows PC and a Linux PC on the same LAN, for machines that share
keyboard and mouse with Synergy 3 (which cannot drag files across). Both machines run the
same program: each one can send files to, and receive files from, its paired partners.
Files land in the Downloads folder; transfers are authenticated and encrypted after a one-time
pairing with an 8-digit code.

**Status:** v0.1.0. Windows <-> Linux, both directions. Not released, so build from source (see
"Build from source"). This is the file-sending part of the `synergy-tools` program (formerly the
separate `synergy-dragndrop`), built from `src/` at the root of the
[synergy-tools](https://github.com/ztancrell/synergy-tools) repo.

The Linux target is Openbox + tint2 on X11 with Thunar and dunst. KDE and GNOME work as extras.

## Install

Superseded: install and autostart scripts were removed; phase 3 adds an installer for synergy-tools. Until then run the binary by hand.

The program needs the GTK 3 runtime and the Ayatana appindicator library for the tray icon:
Debian/Ubuntu `libgtk-3-0` and `libayatana-appindicator3-1`; Arch/CachyOS `gtk3` and
`libayatana-appindicator`. TCP port 24851 must be open inbound in the firewall on both PCs.

The Windows exe has no console of its own, so in a terminal PowerShell may return its prompt
before the command has finished, and `$LASTEXITCODE` is unreliable. Every example below therefore
ends in `| Out-Host`; the alternative is
`Start-Process -Wait -NoNewWindow .\synergy-tools.exe -ArgumentList ...` (or cmd's
`start /wait`). Without a terminal (Send To) results appear as notifications.

### Pairing

Pair once per PC; the pairing is remembered on both PCs. Pairing with another PC adds it to the
partner list, so one PC can have several partners (a Boot Camp Mac with Windows and macOS on one
address counts as two). The two PCs must reach each other on TCP port **24851** (open it in the
firewall).

1. On PC 1 either use the tray menu **Pair…** (the code shows in the drop window's title and in a
   notification, with the command for the other PC; clicking that notification copies the command,
   middle-click on dunst, via `xclip` on Linux) or run `synergy-tools pair`. It prints a
   code like `1234-5678` and the command to run on the other PC. The code works for one attempt
   and expires after 2 minutes.
2. On PC 2 run `synergy-tools pair <PC 1's address> 1234-5678`
   (Windows: `.\synergy-tools.exe pair <address> 1234-5678 | Out-Host`).

Both sides print or show `Paired with <the other PC's name>` and the tray icons turn green. A wrong
code fails on both sides and the code is spent: start again with a new one. Pairing the same PC
again (same name and OS) replaces its old entry instead of adding a second one.

- `synergy-tools partners` lists the partners, one per line: name, OS, address.
- If a partner's address changes, run `synergy-tools partner <name> <new address>` (the secret is
  kept, no new pairing needed).
- `synergy-tools unpair <name>` forgets one partner.
- With several partners, `files send` needs `--to <name>`; a drop on the drop window works only
  while there is exactly one partner (otherwise it fails with an error naming them).

`<name>` matches the partner's name (case-insensitive) or its address. The list is the file
`partners` in `~/.config/synergy-tools/` (Linux, mode 0600) or `%APPDATA%\synergy-tools\`
(Windows), one line per partner: `<id> <address> <secret> <os> <name>`.

## Daily use

- **Drop window:** a small window titled `Drop files to send` sits at the bottom-right of the
  screen (requested 160x160; the window manager may make it larger, e.g. 200x200 on Openbox).
  Drag files onto it (from Explorer, Thunar, ...). The title shows `Release to send`,
  `Sending 2 files…`, then `Sent ✓` or `Not sent`, and a notification gives the result. Closing or
  minimizing the window only hides it to the tray (tray menu: Show drop window); it has no
  taskbar / task-list entry. Without a tray icon, minimize is a normal minimize and the entry stays. On Windows and on Linux/X11 (e.g. Openbox)
  the window stays on top by itself.
- **Right-click:** a Send To entry that runs `synergy-tools files send` on the selected files works
  (results show as notifications); nothing creates one for you this phase (see Install).
- **Tray icon** (status only, not a drop target). Colours: grey = not paired, green = paired and
  idle, blue = sending or receiving, red for 5 seconds after an error. The first menu line (greyed
  out) shows the status, `Paired with <names>` or `Not paired` (the hover tooltip with the same text
  works on Windows only; Linux trays don't get it). Menu: `Hide drop window` / `Show drop window`,
  `Pair…`, `Quit`. If the tray icon can't be created (library missing), a notification says so
  and closing the drop window quits the app. If the receiver thread dies, the app says so and
  exits. Starting a second copy fails with a notification, since the first one holds port 24851.
  On Linux the icon uses libappindicator (`libayatana-appindicator3`, with a fallback to an XEmbed
  icon for panels like tint2 without StatusNotifierItem support).

### Command line

```
synergy-tools                                  tray app (drop window, receives files)
synergy-tools pair                             show a code and wait for the other PC to pair
synergy-tools pair <host> <code>               pair with the PC showing <code> (adds a partner)
synergy-tools partners                         list partners
synergy-tools partner <name> <host>            change a partner's address
synergy-tools unpair <name>                    forget a partner
synergy-tools files send [--to <name>] <file>...  send files to a partner
synergy-tools files listen                     receive files without the tray app
synergy-tools --version
```

On Windows use `.\synergy-tools.exe ... | Out-Host`. `<host>` is an IP address or name,
optionally `:port` (default 24851). `files send` goes to the only partner, or to `--to <name>` when
there are several; it takes files only (folders are refused: zip them first)
and fails at once, within 3 seconds, if the other PC is unreachable; nothing is queued or retried.
If the tray app is already running it holds port 24851, so `pair` and `files listen` say so; use its tray
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

Results show as desktop notifications, since "Send to" has no terminal: `files send` reports success or
the error, and a received transfer shows `N files received from <other PC>` (`1 file received from
...` for one). Opening the Downloads folder (`xdg-open` on Linux, `explorer.exe` on Windows):

- Windows, KDE, GNOME: click the notification.
- dunst (Openbox): left-click only closes the notification; **middle-click opens Downloads**
  (dunst's default `mouse_middle_click = do_action`).

The same click on the **Pair…** notification copies the pairing command to the clipboard (`xclip`
on Linux, `clip` on Windows).

If a notification can't be shown, the reason goes to stderr and the transfer is unaffected.

## Troubleshooting

- **`not sent: other PC unreachable`** (after 3 seconds): the other PC is off, the address in the
  pairing is out of date (`synergy-tools partner <name> <new address>`; a fixed address for it in your
  router avoids this), or its firewall blocks TCP 24851. On Windows, as administrator:
  `New-NetFirewallRule -DisplayName 'synergy-tools (TCP 24851)' -Direction Inbound -Protocol TCP -LocalPort 24851 -Program '<path to>\synergy-tools.exe' -Profile Private,Public -Action Allow`.
  The rule covers only that exe path.
- **`not sent: <name> is a folder, zip it first`**: only files are sent.
- **`not sent: the other PC refused (pairing doesn't match: pair again)`**: `unpair <name>` and pair again.
- **`several partners (...): choose one with --to <name>`**: add `--to <name>`, or `unpair` the
  partners you no longer use.
- **`port 24851 is busy: synergy-tools is probably already running, or the old synergy-dragndrop`**:
  another copy (or the old `synergy-dragndrop`, if it still autostarts) holds the port. Use the tray
  menu **Pair…** or quit it before running `pair` or `files listen` in a terminal.
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

Superseded: install and autostart scripts were removed; phase 3 adds an installer for synergy-tools. Until then run the binary by hand. To remove it, delete the binary and the config folder.

## Build from source

```sh
cargo build --release
cargo test
```

The binary is `target/release/synergy-tools`. Linux needs `libgtk-3-dev`,
`libayatana-appindicator3-dev` and `libasound2-dev` (Arch/CachyOS: `gtk3`,
`libayatana-appindicator` and `alsa-lib`).

Windows exe, cross-built on the Linux PC:

```sh
rustup target add x86_64-pc-windows-gnu
# mingw-w64: Arch `mingw-w64-gcc`; Debian/Ubuntu `gcc-mingw-w64-x86-64`
RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-pc-windows-gnu
```

Copy `target/x86_64-pc-windows-gnu/release/synergy-tools.exe` to the Windows PC. Alternatively run
a native MSVC `cargo build --release` on Windows. A quick check without linking:
`cargo check --target x86_64-pc-windows-gnu`. Commands above run at the repo root. CI
(`.github/workflows/ci.yml`) runs `cargo test` and `cargo build --release` on Ubuntu 22.04 and
Windows for pushes to `main` and all pull requests. There is no release workflow this phase.

## How it works

TCP port 24851, one connection per batch. The first byte says what it is: `P` pairing, `S` session.
Pairing runs SPAKE2 over the 8-digit code (one attempt, 120 s); the two PCs also exchange their
name and OS, and both key confirmations cover them, so a tampered name fails the pairing. Both
sides keep a shared secret and a partner id, the first 8 bytes of SHA-256(`synergy-tools id` +
secret), the same on both PCs. A session starts with `S` and the sender's partner id, which tells
the receiver whose secret to use (several partners can share one address), then a Noise handshake (`Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s`) with that secret as
pre-shared key, then encrypted messages: batch header (count, sender name), then for each file a
header (size, name), data chunks and an end marker; the receiver answers with the number saved
or a failure reason. Frames are a 16-bit length plus bytes. Connect timeout 3 s, stall timeout 30 s.

## Security notes

- Only partners can send: a connection with an unknown partner id or a failed handshake is
  refused and `refused connection from <ip>: not a partner` is printed by `files listen` and `pair` in the
  terminal (the tray app has no log).
- Transfers are encrypted and authenticated, but the 8-digit code is short: pair on a network you
  trust, within the 2-minute window. Anyone who guesses it in the one attempt would be paired, so
  a failed attempt ends pairing mode.
- The secrets sit unencrypted in the `partners` file, readable only by you on Linux. The partner id
  is sent in the clear, so someone on the LAN can tell which partner connects, but not read or
  forge anything.
- Received names are sanitised and files are never overwritten or made executable (Linux).
- LAN only; nothing goes through the internet.

## License

GPL-3.0-or-later, see [LICENSE](../LICENSE).
