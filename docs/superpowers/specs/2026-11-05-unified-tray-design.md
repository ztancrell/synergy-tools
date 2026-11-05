# synergy-tools: one program, one tray icon

Date: 2026-11-05. Status: approved by the user 2026-11-05.

## Intent

Synergy 3 shares keyboard and mouse between two computers but not audio or file
drag-and-drop. `synergy-tools` fills those gaps. Today each gap is filled by its own
program (`synergy-audio`, `synergy-dragndrop`), with its own pairing, startup and controls.
That feels fragmented. The user wants **one program, `synergy-tools`, that replaces both**:
start it on each machine, get one tray icon, control every tool from its menu.

## Current state (as of this design)

The repo was merged from submodules into a monorepo on 2026-11-05: `synergy-audio/` and
`synergy-dragndrop/` are plain folders, each its own Rust crate with its own CI and
release workflows (tags `audio-v*`, `dragndrop-v*`).

- **synergy-audio** (`synergy-audio/src/main.rs`, one file): command-line only.
  `pair` / `pair <host> <code>`, `send <target> [--log path]`, `receive [--buffer-ms n]`.
  Sender is Windows or macOS (captures all system audio, auto-mutes Windows speakers);
  receiver is Linux (PipeWire). Audio over UDP, cpal. Autostart today: systemd user
  service (Linux receiver), scheduled task (Windows sender), none on macOS.
- **synergy-dragndrop** (`src/{gui,pair,transfer,proto,config,store,notify}.rs`): tao +
  tray-icon. Tray menu: status line, "Hide drop window", "Pair…", "Quit". Also a drop
  window and a right-click "Send to" integration. Windows ↔ Linux. Config in
  `<config_dir>/synergy-dragndrop/` (`peer` file).
- **Pairing** is duplicated: both use SPAKE2 + Noise NNpsk0 (snow, spake2, getrandom,
  sha2), each with its own 8-digit code and stored key.

## Goals

1. One executable, `synergy-tools` (`synergy-tools.exe` on Windows), runs on every
   supported machine and replaces both old binaries.
2. One tray icon. Menu (user agreed to this shape):
   - status line (paired with whom, what's running)
   - **Audio** section: streaming state; start/stop; mute toggle on the sending machine
   - **Files** section: show/hide drop window
   - **Pair…** (shared) and **Quit**
3. **Pair once** for both tools: one code, one stored key, used by audio and files.
4. **Audio streams by itself**: when both machines are running and paired, audio starts
   without a click. The tray's stop is a manual override.
5. Existing command-line uses keep working for scripts (e.g. `synergy-tools audio send …`
   or equivalent; exact shape is the builder's call).
6. **Several partners** (user decision): a machine can be paired with more than one other
   machine, e.g. a Linux box paired with both a Mac's Boot Camp Windows and its macOS.
   Pairing again *adds* a partner (it does not replace). One shared peer list replaces
   audio's `senders/` and files' single `peer`. Files are sent to whichever partner is
   online; if several are online, the drop window / "Send to" lets the user pick.
   The tray status line lists online partners.
7. **Platforms: Windows, Linux and macOS** (user decision). macOS gets the full tray:
   Audio (sender) **and Files**, which means porting file-sending (drop window, tray,
   a Finder "send" integration in place of Windows' right-click "Send to") to macOS.
   macOS gets a release artifact and login autostart.

## Non-goals

- New tools or features beyond what the two tools do today (macOS Files, goal 7, is the
  one deliberate exception).
- Changing audio direction (still Windows/macOS sender → Linux receiver).
- A settings window; the tray menu is the whole UI (plus the existing drop window).

## Anti-goals

- Two tray icons, or a launcher that spawns the old programs as separate processes.
- Making the user pair twice.
- Losing the old command-line path for headless/scripted setups.

## Calls made by Claude (not discussed with the user; check these)

- **Mute toggle** = turn the existing Windows auto-mute (speakers muted while Linux is
  playing, restored when the link drops) on or off. Saved across restarts. It is not a
  manual speaker mute and keeps the rule "never touch a mute we didn't set". Not shown on
  macOS, which has no auto-mute today.
- **Stop** shows on both machines. Pressing it on either side stops the stream (a new
  control message in the audio protocol), so Windows unmutes. It lasts until "Start" or
  the next launch of the program; it is not saved, so after a reboot audio streams by
  itself again (goal 4).
- **Migration:** no compatibility with the old binaries or their keys. Users update both
  machines and pair once more. Installers/autostart setup remove the old programs'
  autostart entries (systemd service, XDG/openbox autostart, Windows scheduled tasks,
  Send To shortcut), so the old programs can't hold ports 24850/24851 or show a second
  tray icon.
  On Windows (user request, 2026-11-05; done by hand on the Boot Camp machine that day),
  the phase 3 installer removes, in this order: run the old `synergy-audio.exe unmute` if
  it exists; stop and unregister scheduled tasks `synergy-audio` and `synergy-dragndrop`
  and kill those processes; delete `%LOCALAPPDATA%\synergy-audio`,
  `%LOCALAPPDATA%\synergy-dragndrop`, `%APPDATA%\synergy-audio`, `%APPDATA%\synergy-dragndrop`
  and `%APPDATA%\Microsoft\Windows\SendTo\Send to other PC.lnk`; remove firewall rule
  `synergy-dragndrop (TCP 24851)` (needs admin).
- **Linux autostart** = launch the tray at desktop login, which replaces the systemd
  receiver service. Headless audio stays possible through the command line
  (`audio receive`), documented but not installed by default; it must not run at the
  same time as the tray copy.

## Left to the builder

- Crate layout (one workspace with shared `pairing`/`crypto` crate + binary, or a single
  crate) and how much to refactor the two tools' code.
- Exact CLI subcommand names, as long as old functions remain reachable.
- How "audio streams by itself" discovers/decides the sender vs receiver role (it follows
  from the platform: Linux receives, Windows/macOS sends).
- Updating CI/release workflows to build and release `synergy-tools` (a single tag scheme
  such as `v*` replacing the per-tool tags), and docs/READMEs in the same commits.
