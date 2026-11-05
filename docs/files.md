# synergy-tools: files

Sends files between Windows, macOS and Linux PCs on the same LAN, for machines that share
keyboard and mouse with Synergy 3 (which cannot drag files across). Both machines run the
same program: each one can send files to, and receive files from, its paired partners.
Files land in the Downloads folder; transfers are authenticated and encrypted after a one-time
pairing with an 8-digit code.

**Status:** v0.1.0, part of `synergy-tools` (the root crate of this repo; code in `src/`). Windows, macOS and Linux, any pair, both directions. Not released yet: build from source (see "Build from source").

The Linux target is Openbox + tint2 on X11 with Thunar and dunst. KDE and GNOME work as extras.

## Install

Superseded: install and autostart scripts were removed; phase 3 adds an installer for synergy-tools. Until then run the binary by hand.

The program needs the GTK 3 runtime and the Ayatana appindicator library for the tray icon on Linux:
Debian/Ubuntu `libgtk-3-0` and `libayatana-appindicator3-1`; Arch/CachyOS `gtk3` and
`libayatana-appindicator`.

### macOS

Run it from Terminal: `chmod +x synergy-tools && xattr -d com.apple.quarantine synergy-tools && ./synergy-tools`,
or `nohup ./synergy-tools >/dev/null 2>&1 &` to be able to close the Terminal window. The icon sits
in the menu bar, with no Dock icon; the drop window has no minimize button. If the macOS firewall
asks, allow incoming connections (needed for TCP 24851). A Finder send entry comes with phase 3's
installer; until then drop files on the window or use `synergy-tools files send`.

The first time a file arrives, macOS may ask "Terminal would like to access files in your
Downloads folder". Allow it: otherwise receives fail with "Operation not permitted" and the
sender sees "not sent".

## Pairing

Pair once; the pairing is remembered on both PCs. Pairing with another PC adds it to the list
of partners; pairing again with the same PC (same name and OS) replaces its entry. The two PCs
must reach each other on TCP port
**24851** (open it in your firewall).

**Boot Camp:** pair each OS of the Mac once (the Linux side then has two partners, same IP is
fine). Only the booted one shows online. On the command line name them `<name>/<os>`, e.g.
`synergy-tools unpair MBP/windows`.

1. On PC 1 either use the tray menu **Pair…** (the code shows in the drop window's title and in a
   notification, with the command for the other PC; clicking that notification copies the command,
   middle-click on dunst, via `xclip` on Linux; on macOS the notification has no click action, so
   copy the command from its text) or run `synergy-tools pair`. It prints a
   code like `1234-5678` and the command to run on the other PC. The code works for one attempt
   and expires after 2 minutes.
2. On PC 2 run `synergy-tools pair <PC 1's address> 1234-5678`
   (Windows: `.\synergy-tools.exe pair <address> 1234-5678 | Out-Host`).

Both sides print or show `Paired with <the other PC's name>` and the tray icons turn green. A wrong
code fails on both sides and the code is spent: start again with a new one.

- `synergy-tools partners` lists the partners (`name  os  host`, one per line) or `Not paired.`
- If a partner's address changes, run `synergy-tools partner <name> <new address>` (the secret is
  kept). `<name>` matches the partner's name (case doesn't matter) or its current address. If it
  matches several partners (say one PC paired from both Windows and Linux), the command fails and
  lists them; pick one as `<name>/<os>`, e.g. `pc/windows`. The same goes for `unpair` and `--to`.
- `synergy-tools unpair <name>` forgets that partner.
- Files go to the partner that is online (running synergy-tools). With several online, `files send`
  needs `--to <name>` and the drop window shows a picker. Superseded: the drop window used to need
  exactly one paired partner.

The partners are the file `partners` in `~/.config/synergy-tools/` (Linux, mode 0600) or
`%APPDATA%\synergy-tools\` (Windows) or `~/Library/Application Support/synergy-tools/` (macOS),
one line per partner:
`<id> <host> <secret> <os> <name>`.

## Daily use

On macOS, notifications come from `osascript` and have no click action. macOS may list them under
Script Editor in System Settings → Notifications; allow them there if none appear.

- **Drop window:** a small window titled `Drop files to send` sits at the bottom-right of the
  screen (requested 160x160; the window manager may make it larger, e.g. 200x200 on Openbox).
  Drag files onto it (from Explorer, Thunar, ...). They go to the partner that is online; with
  several online a menu of their names pops up and the files go to the one you click (dismiss it
  to send nothing; a new drop replaces the held files). With nobody online the notification says
  `not sent: no partner online` and nothing is sent. The title shows `Release to send`,
  `Sending 2 files…`, then `Sent ✓` or `Not sent`, and a notification gives the result. Closing or
  minimizing the window only hides it to the tray (tray menu: Show drop window); it has no
  taskbar / task-list entry. Without a tray icon, minimize is a normal minimize and the entry
  stays. On Windows and on Linux/X11 (e.g. Openbox) the window stays on top by itself. macOS has
  no minimize button (see "macOS" above).
- **Right-click:** the Send To entries (Windows Explorer, Thunar, Dolphin, Nautilus) come back
  with phase 3's installer. Until then use the drop window or `synergy-tools files send`.
  Superseded: they came from the removed install scripts.
- **Tray icon** (status only, not a drop target). Colours: grey = not paired, green = paired and
  idle, blue = sending or receiving, red for 5 seconds after an error. The first menu line (greyed
  out) shows the status, `Online: <names>`, `No partner online` or `Not paired`, refreshed every
  5 seconds (the hover tooltip with the same text
  works on Windows and macOS; Linux trays don't get it). Menu: status line, the audio section
  ([docs/audio.md](audio.md#in-the-tray)), `Hide drop window` / `Show drop window`, `Pair…`,
  `Quit`. If the tray icon can't be created (library missing), a notification says so
  and closing the drop window quits the app. If the receiver thread dies, the app says so and
  exits. Starting a second copy fails with a notification, since the first one holds port 24851.
  On Linux the icon uses libappindicator (`libayatana-appindicator3`, with a fallback to an XEmbed
  icon for panels like tint2 without StatusNotifierItem support).

### Command line

```
synergy-tools                         tray app (drop window, receives files)
synergy-tools pair                    show a code and wait for the other PC to pair
synergy-tools pair <host> <code>      pair with the PC showing <code> (adds a partner)
synergy-tools partners                list paired PCs
synergy-tools partner <name> <host>   change a partner's address
synergy-tools unpair <name>           forget a partner
synergy-tools files send [--to <name>] <file>...
                                      send files to the online partner (--to if several are online)
synergy-tools files listen            receive files without the tray app
synergy-tools --version
```

On Windows use `.\synergy-tools.exe ... | Out-Host`. `<host>` is an IP address or name,
optionally `:port` (default 24851). `files send` takes files only (folders are refused: zip them first)
and first checks which partners are online (about a second): with nobody online it fails with
`not sent: no partner online`, with several it asks for `--to <name>` (`--to` picks among the
online ones). Nothing is queued or retried.
If the tray app is already running it holds port 24851, so `pair` and `files listen` say so; use its tray
menu or quit it first.

## Where files go

Received files are saved in your Downloads folder.

- A file is written under a temporary name (`.name.xxxxxxxx.part`, hidden on Linux and macOS, visible on
  Windows) and only gets its real name once it has arrived completely, so you never see half a
  file. A crash mid-receive can leave one behind; it is safe to delete and never looks like a
  complete file.
- An existing file is never replaced: a second `report.pdf` is saved as `report (1).pdf`,
  then `report (2).pdf`, and so on.
- File names from the other PC are cleaned up: folders in the name are dropped, and characters
  Windows does not allow (`< > : " | ? *`) become `_`.
- On Windows, received files are marked as downloaded from the internet (Mark of the Web,
  `ZoneId=3`), so SmartScreen may ask before you open an executable. On macOS, received files carry the
  `com.apple.quarantine` mark, so Gatekeeper checks them before opening. On Linux and macOS,
  received files are never executable.

## Notifications

Results show as desktop notifications, since "Send to" has no terminal: `files send` reports success or
the error, and a received transfer shows `N files received from <other PC>` (`1 file received from
...` for one). Opening the Downloads folder (`xdg-open` on Linux, `explorer.exe` on Windows):

- Windows, KDE, GNOME: click the notification.
- macOS: no click action (see Daily use).
- dunst (Openbox): left-click only closes the notification; **middle-click opens Downloads**
  (dunst's default `mouse_middle_click = do_action`).

The same click on the **Pair…** notification copies the pairing command to the clipboard (`xclip`
on Linux, `clip` on Windows).

If a notification can't be shown, the reason goes to stderr and the transfer is unaffected.

## Troubleshooting

- **`not sent: other PC unreachable`** (after 3 seconds): the other PC is off, the address in the
  pairing is out of date (`synergy-tools partner <name> <new address>`; a fixed address for it in your
  router avoids this), or its firewall blocks TCP 24851. Open TCP 24851 there (on Windows, `New-NetFirewallRule -DisplayName 'synergy-tools (TCP 24851)' -Direction Inbound -Protocol TCP -LocalPort 24851 -Profile Private,Public -Action Allow` as administrator).
- **`not sent: <name> is a folder, zip it first`**: only files are sent.
- **`not sent: the other PC refused (pairing doesn't match: pair again)`**: pair again; pairing with the same PC (same name and OS) replaces its
  entry, no unpair needed.
- **`not sent: no partner online`**: no partner answered within a second: start synergy-tools on
  the other PC, or check its address and firewall (see `other PC unreachable`).
- **`several partners online (...): use --to <name>`**: name the receiver with `--to`.
- **`... partners is damaged: delete it and pair again`**: the partners file was edited or cut
  short; delete it and pair again.
- **`port 24851 is busy: synergy-tools is probably already running`**: the tray app holds the
  port. Use its tray menu **Pair…** or quit it before running `pair` or `files listen` in a terminal.
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

Superseded: uninstall scripts were removed along with the installers; delete the binary and, if you want, the config folder.

## Build from source

```sh
cargo build --release
cargo test
```

The binary is `target/release/synergy-tools`. Linux needs `libgtk-3-dev`,
`libayatana-appindicator3-dev` and `libasound2-dev` (Arch/CachyOS: `gtk3`,
`libayatana-appindicator` and `alsa-lib`). Windows: a
native MSVC `cargo build --release`. macOS: `xcode-select --install` and rustup, then the same
commands. CI (`.github/workflows/ci.yml`) runs `cargo test` and `cargo build --release` on
Ubuntu 22.04, Windows and macOS (the macOS job builds a universal binary, see the README).

## How it works

TCP port 24851, one connection per batch. The first byte says what it is: `P` pairing, `S` session,
`H` presence ping. Every 5 seconds the tray pings each partner (`H`, partner id, the same Noise
handshake, then close; 1 s timeout); the partners that complete it are "online". The receiver
answers a ping silently: no notification, and an unknown id just closes the connection.
Pairing runs SPAKE2 over the 8-digit code (one attempt, 120 s); each side also sends its OS and
name, both confirmations cover both names, and both sides keep a shared secret plus the other's
name. The partner id is the first 8 bytes of `SHA256("synergy-tools id" ‖ secret)`; a session
starts with `S` and the sender's partner id, so the receiver picks that partner's secret.
A session is a Noise handshake (`Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s`) with that secret as
pre-shared key, then encrypted messages: batch header (count, sender name), then for each file a
header (size, name), data chunks and an end marker; the receiver answers with the number saved
or a failure reason. Frames are a 16-bit length plus bytes. Connect timeout 3 s, stall timeout 30 s.

## Security notes

- Only a paired PC can send: a connection with an unknown partner id or a failed handshake is
  refused and `refused connection from <ip>: not a paired PC` is printed by `files listen` and `pair` in the
  terminal (the tray app has no log).
- Transfers are encrypted and authenticated, but the 8-digit code is short: pair on a network you
  trust, within the 2-minute window. Anyone who guesses it in the one attempt would be paired, so
  a failed attempt ends pairing mode.
- The partner id is not secret; it only selects which secret the handshake must prove.
- Names are bound into the pairing confirmation, so they can't be changed in transit. A name is
  what the other PC says it is (its computer name), not proof of identity.
- The secrets sit unencrypted in the `partners` file, readable only by you on Linux and macOS.
- Received names are sanitised and files are never overwritten or made executable (Linux and macOS).
- Received files are marked as downloaded (quarantine on macOS, Mark of the Web on Windows); a receive
  fails rather than keep an unmarked file.
- LAN only; nothing goes through the internet.

## License

GPL-3.0-or-later, see [LICENSE](../LICENSE).
