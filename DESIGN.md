# synergy-dragndrop — design

## The idea

A companion to [synergy-audio](../synergy-audio): two PCs on the same LAN share keyboard and
mouse with Synergy 3, which can't move files between them. This program moves files
between them in **both directions**: a Mac running Windows through Boot Camp, and a Linux PC
(CachyOS, Openbox window manager with a tint2 panel on X11, Thunar file manager, dunst
notifications). You drop files on a small window, or right-click
them and pick "Send to other PC", and they show up in the other machine's Downloads folder.

Typical files: `.exe`, `.zip`, screenshots. Nothing over about 100 MB.

## Goals

- Send one or more files from either machine to the other with one gesture.
- Two ways to send, on both OSes:
  - **Drop window:** a small always-on-top square near a screen edge. Drag files onto it
    to send them.
  - **Right-click "Send to other PC":** in the Windows Explorer *Send to* menu, and in
    Thunar's *Send To* menu on Linux (Dolphin/Nautilus entries are cheap extras).
- **Tray icon** on both sides, for status (connected or not, a transfer in progress) and a
  small menu: show or hide the drop window, pairing, quit. Tray icons can't accept dropped
  files on Windows or Linux, so the tray is not a drop target.
- **Receiving:** files are saved to the user's Downloads folder. A desktop notification says
  "N files received from <other PC>", and clicking it opens the folder. If a file with that
  name already exists, add ` (1)`, ` (2)` and so on. Never overwrite.
- **Pairing:** pair once with a short code. After that, each machine accepts files only from
  its paired partner. Anything else on the LAN gets refused.
- Starts at login on both machines, like synergy-audio's autostart scripts.
- Prebuilt Linux x86_64 and Windows x86_64 releases, same CI and release style as
  synergy-audio.

## Non-goals (v1)

- **Dragging across the screen edge with the Synergy cursor.** This was the original idea,
  and it's dropped for v1. On Wayland, a background program can't watch or start global
  drags, and Synergy 3 doesn't help. Decided 2026-11-04.
- Folders. Dropping a folder gives a clear "zip it first" message. Users already send zips.
- Large-file features: resume, multi-GB tuning, progress bars beyond a simple
  "sending…/sent" state. A ~100 MB file over the LAN takes seconds.
- macOS (the Mac runs Windows), more than two machines, clipboard sharing, internet use.

## Anti-goals (a failure even if it works)

- A file silently overwritten or lost. A half-received file left looking complete. Partial
  files are written under a temporary name and renamed only after the full file is
  received and checked.
- Any machine on the LAN able to push files onto either PC.
- Fiddly setup. Once paired, it should just work at every login.

## Constraints

- Rust, matching synergy-audio. A single program with the same role on both sides: each
  machine both sends and receives.
- LAN only. The user enters the other PC's address, as in synergy-audio (IP or hostname).
  The program listens on a TCP port next to synergy-audio's (synergy-audio uses UDP 24850,
  so use 24851). The autostart and install scripts open it in the firewall the way
  synergy-audio's do.
- Linux side must work on the user's actual desktop: Openbox + tint2 on X11 (tint2's tray is
  the older XEmbed kind, not KDE/GNOME's StatusNotifierItem). KDE/GNOME Wayland support is
  nice-to-have, not required.
  Superseded: the first draft assumed KDE Plasma on Wayland; checked 2026-11-04, it's Openbox/X11.
- Windows 10/11, 64-bit.
- Pairing and file transfer go over an authenticated connection, encrypted too (my call:
  it's cheap and files may be private). The library and protocol are up to the builder.
- Docs change in the same commit as the code they describe.

## Left to the builder

- GUI, tray and notification crates; the wire protocol; how the pairing code is entered and
  stored (config file location per OS).
- Whether "Send to" runs a short-lived `synergy-dragndrop send <files…>` that connects to the
  peer directly, or hands the files to the running instance.
- Drop window size, look and default position.

## Decided

- 2026-11-04: Windows (Boot Camp) ↔ Linux, both directions.
- 2026-11-04: Drop window + right-click "Send to" instead of a cross-edge drag. The tray is
  for status only.
- 2026-11-04: Received files go to Downloads, with a notification.
- 2026-11-04: Pair once with a short code; refuse everyone else. (My call, user didn't
  object.)
- 2026-11-04: No folders in v1; files up to ~100 MB.
- 2026-11-04: Received files on Windows get the "downloaded from the internet" mark (Mark of
  the Web: a `Zone.Identifier` stream with `ZoneId=3`). Windows then shows its own SmartScreen
  warning when one is run, and "More info → Run anyway" lets the user through. No warning
  dialog of our own. On Linux, received files are saved without the execute bit, which
  covers the same ground.
- 2026-11-04: If the other PC is unreachable, a send fails right away with a "not sent:
  other PC unreachable" notification. No queue, no retry. (My call.)

- 2026-11-04: Windows setup asks for admin once (one UAC prompt) to add an inbound firewall
  rule for TCP 24851, covering Private and Public networks (Boot Camp Wi-Fi is often
  "Public"). synergy-audio's Windows script never needed this, since that side only sends.

- 2026-11-04: Linux target is Openbox + tint2 on X11 with Thunar (found by checking the running
  session). Tray uses XEmbed-compatible appindicator fallback; right-click entry goes in
  Thunar's Send To menu; autostart also via `~/.config/openbox/autostart`. (My call.)

- 2026-11-04: "Send to" runs a short-lived `synergy-dragndrop send <files...>` that connects to
  the peer directly (no hand-off to the running instance); the result also goes to a notification.
- 2026-11-04: Crates: `tao` (drop window), `tray-icon` with its default libappindicator backend
  plus XEmbed fallback for tint2 (not `ksni`, which needs StatusNotifierItem), `notify-rust`
  (Linux), `tauri-winrt-notification` (Windows).
  Superseded: the task list said tray-icon/ksni; the builder chose tray-icon only (see src/gui.rs).
- 2026-11-04: Pairing: one PC shows an 8-digit code (tray "Pair..." or `synergy-dragndrop pair`),
  the other runs `synergy-dragndrop pair <host> <code>`; one attempt, 120 s. The result is a
  `peer` file (`<host> <secret-hex>`, mode 0600 on Linux) in the config dir
  (`~/.config/synergy-dragndrop/` or `%APPDATA%\synergy-dragndrop\`).
- 2026-11-05: Linux autostart: the XDG entry `~/.config/autostart/synergy-dragndrop.desktop` when
  `openbox-xdg-autostart` exists, else a marked line (`# synergy-dragndrop`) in
  `~/.config/openbox/autostart`; never both. Right-click entry in Thunar's Send To
  (`~/.local/share/Thunar/sendto`), plus Dolphin and Nautilus. Windows: scheduled task and a
  Send To shortcut.
  Superseded: the 2026-11-04 Linux line said "autostart also via ~/.config/openbox/autostart";
  it is one mechanism or the other.
- 2026-11-05: Known v1 limitations, accepted: (a) if a later file in a batch fails, earlier files
  stay saved although the sender sees "not sent" for the whole batch; (b) a peer IP change needs
  `synergy-dragndrop peer <address>` (or a fixed address in the router); (c) the Linux tray has
  no tooltip, so the status shows as the first (greyed) tray menu item.

- 2026-11-05: Change of direction: synergy-dragndrop will be merged into synergy-audio as one
  program (one binary, tray, pairing, installer). The merged design is still to be worked out;
  this project stays as built until then.

## Open questions

- (none)
