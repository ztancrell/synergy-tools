# synergy-audio

[![CI](https://github.com/ztancrell/synergy-audio/actions/workflows/ci.yml/badge.svg)](https://github.com/ztancrell/synergy-audio/actions/workflows/ci.yml)

Shares audio between two machines that already share keyboard and mouse with Synergy 3
(which carries no audio). One machine captures everything it plays and streams it over UDP
on the LAN; the other plays it on its default output, with a small jitter buffer for low
latency.

**Status: early (v0.2.0).** Windows → Linux only, terminal only (plus optional autostart:
a systemd user service for the Linux receiver, a scheduled task for the Windows sender).
Prebuilt downloads on the [Releases page](https://github.com/ztancrell/synergy-audio/releases).
No GUI, no Synergy integration, no switching direction, no self-update yet.

## Download and install

You need a Windows PC (the one whose audio you want to hear) and a Linux PC with PipeWire
(the one with your headphones) on the same LAN. Both sound devices must run at 48 kHz.
From the [Releases page](https://github.com/ztancrell/synergy-audio/releases), download
the two archives of the newest release:

- `synergy-audio-vX.Y.Z-linux-x86_64.tar.gz` for the Linux PC (built on Ubuntu 22.04, runs
  on most current distros).
- `synergy-audio-vX.Y.Z-windows-x86_64.zip` for the Windows PC (64-bit Windows 10/11).

**Linux (receiver), first:**

```sh
tar xf synergy-audio-vX.Y.Z-linux-x86_64.tar.gz
cd synergy-audio-vX.Y.Z
./install-autostart.sh
```

It copies the bundled program to `~/.local/bin/synergy-audio` and installs a systemd user
service that starts the receiver at every login (details: steps 13-17 below). If ufw or
firewalld is active and UDP 24850 isn't open yet, it opens it and asks for your sudo
password for that only. Note the Linux PC's LAN address (`ip -4 addr`, e.g.
`192.168.1.20`).

**Windows (sender):** right-click the zip → **Extract All**, open the extracted folder,
click in the address bar, type `powershell` and press Enter. Then, with your Linux PC's
address:

```
powershell -ExecutionPolicy Bypass -File .\install-autostart.ps1 -Desktop 192.168.1.20
```

Always pass `-Desktop`: without it the sender goes to the author's address. It copies the
program to `%LOCALAPPDATA%\synergy-audio` and starts it at every login, hidden (details:
steps 19-23 below). If Windows shows "Windows protected your PC" when you run
`synergy-audio.exe` yourself (the program is unsigned), click **More info → Run anyway**.
Windows only sends, so its firewall needs no change.

Play something on Windows: it should come out of the Linux PC, and the Windows speakers
mute while it does ([Auto-mute](#auto-mute-of-the-macs-speakers)). To check, see steps
14 and 20 below. To remove: `./uninstall-autostart.sh` on Linux,
`powershell -ExecutionPolicy Bypass -File .\uninstall-autostart.ps1` on Windows.
To update: download the new release and run the same install commands again.

Optional: check the downloads against `SHA256SUMS` from the same release page:
`sha256sum -c --ignore-missing SHA256SUMS` on Linux, `Get-FileHash <file>` on Windows
(compare by eye).

To build from source instead, follow [Setup, step by step](#setup-step-by-step).

## Author's setup

What this is built and tested on:

- **Main machine: CachyOS Linux desktop.** Wired Ethernet (`192.168.254.90`), headphones
  plugged in. Synergy 3 server: its keyboard and mouse control both machines. Runs
  `synergy-audio receive`.
- **Gaming machine: MacBook Pro 2019** (Intel i9, Radeon Pro 5500M 8 GB, 32 GB RAM, 1 TB)
  running **Windows 10 through Boot Camp** (with FlexTPM), on Wi-Fi. Synergy 3 client. Plays
  **Battlefield 6**. Runs `synergy-audio send`.

```
keyboard + mouse:  CachyOS desktop --(Synergy 3)--> MacBook Pro (Windows 10)
game audio:        MacBook Pro (Windows 10) --(synergy-audio, UDP 24850)--> CachyOS desktop --> headphones
```

## Setup, step by step

Exactly the steps used to get the author's setup running: a CachyOS Linux desktop (wired,
`192.168.254.90`, user `zpt`, headphones plugged in) receiving audio from a MacBook Pro
running Windows 10 through Boot Camp (Wi-Fi). Swap in your own IP address and username.

### One-time setup on Linux

1. Install Rust (`rustup`) and the Windows cross-compiler (`mingw-w64-gcc`), then add the
   Windows target:
   ```sh
   rustup target add x86_64-pc-windows-gnu
   ```
2. Open the audio port in the firewall (ufw). If you'll use Linux autostart (step 13),
   skip this: `install-autostart.sh` opens the port itself when it isn't open yet. By hand:
   ```sh
   sudo ufw allow 24850/udp
   ```
3. Turn on SSH so Windows can copy the program from this machine, and let it through the
   firewall:
   ```sh
   sudo systemctl start sshd
   sudo ufw allow ssh
   ```
   `start` lasts until the next reboot; run it again after rebooting (or use
   `sudo systemctl enable --now sshd` to keep it on). To turn SSH off again later:
   ```sh
   sudo systemctl stop sshd
   sudo ufw delete allow ssh
   ```
4. Find this machine's IP address (the one on your LAN, e.g. `192.168.254.90`):
   ```sh
   ip -4 addr
   ```

### First run (manual)

5. On Linux, build and start the receiver:
   ```sh
   cd ~/projects/synergy-audio && cargo run --release -- receive
   ```
6. On Windows, open PowerShell and go to your Downloads folder (not `C:\Windows\system32`,
   where an admin PowerShell starts):
   ```
   cd $HOME\Downloads
   ```
7. Copy the program from Linux. The command is **`scp`**, not `cp` (in PowerShell `cp` is a
   local copy and fails with "Cannot find drive"):
   ```
   scp zpt@192.168.254.90:projects/synergy-audio/target/x86_64-pc-windows-gnu/release/synergy-audio.exe .
   ```
   The first time, it asks whether to trust the host key: type `yes`. Then enter your Linux
   password.
8. Start the sender, pointing at the Linux machine's IP:
   ```
   .\synergy-audio.exe send 192.168.254.90
   ```
   If Windows shows "Windows protected your PC" (the program is unsigned), click
   **More info → Run anyway**. If either side exits with a 48 kHz error, set that sound
   device to 48000 Hz as the message says.
9. Play something on Windows and listen on Linux. For a clean test, play one continuous song
   for about 30 seconds and read both sides' status lines (see [Run](#run)). Packets/s drops
   to 0 whenever nothing is playing; that is normal.

### Every update after that (dev scripts)

10. On Windows, fetch the update script once, into Downloads:
    ```
    cd $HOME\Downloads
    scp zpt@192.168.254.90:projects/synergy-audio/update.ps1 .
    ```
11. On Linux, stop any running receiver (Ctrl+C), then build both versions and start the
    receiver (40 ms buffer suits the Mac's Wi-Fi better than the 20 ms default):
    ```sh
    cd ~/projects/synergy-audio && ./dev.sh --buffer-ms 40
    ```
    Once Linux autostart is installed (step 13), `./dev.sh` updates and restarts the
    service instead and shows its log; see step 16.
12. On Windows, stop any running sender (Ctrl+C), then fetch the newest build and start
    sending (it asks for your Linux password):
    ```
    powershell -ExecutionPolicy Bypass -File .\update.ps1
    ```

### Start the receiver automatically at Linux login (optional)

After this, the receiver runs by itself, with no terminal, as a systemd **user** service
(it has to run in your login session to play through PipeWire): it starts when you log
in, including after a reboot, and restarts 3 s after a crash. Its status lines go to the
journal instead of a terminal.

13. On Linux, install it (in a source checkout it builds the release version first; in an
    extracted release folder it installs the bundled program):
    ```sh
    cd ~/projects/synergy-audio && ./install-autostart.sh
    ```
    It copies the program to `~/.local/bin/synergy-audio`, writes
    `~/.config/systemd/user/synergy-audio.service` (runs `synergy-audio receive --buffer-ms
    40`), enables and starts it. A receiver you started by hand in a terminal holds the
    audio port, so the script stops it (it says so). If ufw (or firewalld) is active and
    UDP 24850 isn't open yet, it opens it and **asks for your sudo password** for that only;
    if the port is already open it doesn't ask. For another buffer:
    `./install-autostart.sh --buffer-ms 60`. Rerunning without `--buffer-ms` keeps the
    installed value.
14. Check it's running:
    ```sh
    systemctl --user status synergy-audio
    ```
    It should say `active (running)`. Watch its status lines live (Ctrl+C closes only the
    log view; the receiver keeps running):
    ```sh
    journalctl --user -u synergy-audio -f -o cat
    ```
    Play something on Windows: you should see `receive: audio arriving from ...` and then
    the 5-second status lines.
15. Reboot and log in: the receiver is running again by itself (check as in step 14).
16. Updates now: run step 11 as before (`./dev.sh`, no need to stop anything first). It
    sees the service, builds both versions, copies the new program to `~/.local/bin`,
    restarts the service and follows its log (Ctrl+C closes only the log view). The buffer
    stays as installed unless you pass one: `./dev.sh --buffer-ms 60` changes it for good.
17. To turn autostart off:
    ```sh
    ./uninstall-autostart.sh
    ```
    It stops and disables the service and removes its unit file. It leaves
    `~/.local/bin/synergy-audio` and the firewall rule in place and prints the commands to
    remove them. After that, `./dev.sh` goes back to running the receiver in the terminal.

### Start the sender automatically at Windows login (optional)

After this, the sender starts by itself, with no window, every time you log in to
Windows, including after a reboot. It keeps sending to `192.168.254.90` and writes its
status to a log file instead of a window. Together with Linux autostart (steps 13-17)
nothing needs starting by hand on either side; without it, start the Linux receiver by
hand (step 11, or `cargo run --release -- receive`).

18. On Windows, fetch the two autostart scripts once, into the same Downloads folder that
    holds `synergy-audio.exe` (it asks for your Linux password each time):
    ```
    cd $HOME\Downloads
    scp zpt@192.168.254.90:projects/synergy-audio/install-autostart.ps1 .
    scp zpt@192.168.254.90:projects/synergy-audio/uninstall-autostart.ps1 .
    ```
19. Close any sender you started by hand (Ctrl+C in its window), then install. A normal
    (not administrator) PowerShell is enough:
    ```
    powershell -ExecutionPolicy Bypass -File .\install-autostart.ps1
    ```
    It copies `synergy-audio.exe` to `C:\Users\bootcamp\AppData\Local\synergy-audio\`
    (`$env:LOCALAPPDATA\synergy-audio`), registers a Task Scheduler task named
    `synergy-audio` that runs it 10 s after you log in (only while you're logged on, so it
    can capture your audio), and starts it right away. To send to another IP:
    `.\install-autostart.ps1 -Desktop 192.168.1.20`. If it fails with "Access is denied",
    open PowerShell as administrator (right-click → **Run as administrator**), `cd` to
    `C:\Users\bootcamp\Downloads`, and run the same command again.
20. Check it's running: Task Manager → **Details** tab → `synergy-audio.exe` is listed. Its
    log is `C:\Users\bootcamp\AppData\Local\synergy-audio\synergy-audio.log`; show the last
    lines with:
    ```
    Get-Content $env:LOCALAPPDATA\synergy-audio\synergy-audio.log -Tail 5
    ```
    You should see `send: 200 packets/s ...` lines while something plays. Start the receiver
    on Linux and listen: the lines then end in `receiver ok (...), muted by us`.
21. Reboot Windows and log in: after about 10 s the sender is running again by itself
    (check as in step 20). Nothing to start by hand on Windows.
22. Updates now: step 11 on Linux, then step 12 on Windows as before. `update.ps1` sees the
    task, stops it, copies the new build into `$env:LOCALAPPDATA\synergy-audio\`, restarts
    the task and prints the log's last lines. It no longer runs a sender in the PowerShell
    window, so there is nothing to Ctrl+C first. (Steps 18-19 aren't needed again unless
    the autostart scripts themselves change; then fetch and rerun them.)
23. To turn autostart off:
    ```
    powershell -ExecutionPolicy Bypass -File .\uninstall-autostart.ps1
    ```
    It removes the task, stops the running sender and unmutes the Mac's speakers if the
    sender had muted them (see [Auto-mute](#auto-mute-of-the-macs-speakers)). It leaves the folder
    `$env:LOCALAPPDATA\synergy-audio` (program and log) in place and prints the command to
    delete it. After that, `update.ps1` goes back to running the sender in its window.

### Auto-mute of the Mac's speakers

The sender copies what Windows plays; it doesn't redirect it, so without muting you'd hear
the sound from both the Mac's speakers and your headphones. **The sender now mutes Windows
by itself, but only while the Linux receiver is actually receiving**, so you never end up
with no sound anywhere. Nothing to set up:

- Every 500 ms the sender sends a small keepalive; the receiver answers each one with an
  ack. While the last ack is less than 1.5 s old, the sender keeps the Windows default
  speakers muted (the taskbar speaker icon shows muted). When acks stop (receiver closed,
  network down), it unmutes them within about 2 s.
- **Your own mute is respected:** if the speakers are already muted when the sender would
  mute them, it leaves them alone and never unmutes them. If you unmute while the receiver
  is running, the sender mutes again within half a second; close the receiver (or the
  sender) to get the speakers back.
- **If the sender is killed** (Task Manager, `Stop-Process`, stopping the scheduled task,
  a crash), it can't unmute. It leaves a marker file,
  `%LOCALAPPDATA%\synergy-audio\muted-by-us`, while it has the speakers muted; the next
  start of the sender (autostart at login, `update.ps1`) sees it and unmutes first, and
  `uninstall-autostart.ps1` runs `synergy-audio.exe unmute`, which does the same. Ctrl+C
  or closing the sender's window unmutes right away. If the speakers ever stay muted with
  no sender running, run `synergy-audio.exe unmute` or just unmute with the taskbar icon.

This works because on the author's MacBook Pro capture happens before the volume control:
muted speakers go silent and the headphones keep playing. If muting silences your
headphones too, your sound driver applies the volume before capture, and auto-mute will
cut your stream while the receiver runs; you'd see `muted by us` in the status line and
hear nothing. (There's no switch to turn auto-mute off yet; say so if you need one.)

### Volume and crackles

**Volume.** Because capture happens before the volume control, the main Windows volume slider no longer changes what you
hear: the sender always gets the sound at full level, so it can be very loud. Turn it down
with any of these:

- On Linux: the system volume, or just this program's stream in your sound settings or
  `pavucontrol` (Playback tab; it may show as an ALSA stream).
- On Windows: the app's own volume (YouTube's player slider, the game's volume setting) or
  its slider in the Volume Mixer.
- Your headphones' own volume control.

The program has no volume control of its own yet.

If it crackles, watch the receiver's underrun count and restart it with a bigger buffer
(`./dev.sh --buffer-ms 60`). Wi-Fi on the sending machine is the main cause; a wired
connection allows a smaller buffer and less delay.

## How it works

- Format: uncompressed 16-bit (i16) stereo, 48 kHz. Both devices must run at 48 kHz (no
  resampling); otherwise it exits with an error that says how to change the rate.
- One UDP audio packet = 8-byte sequence number (u64, little-endian) + 5 ms of audio
  (240 frames = 960 bytes). Packets that arrive late or twice are dropped.
- Control packets are exactly 4 bytes, a length no audio packet can have (audio is 8 bytes
  plus whole 4-byte stereo frames): keepalive `SAka` (ASCII), sent by the sender every
  500 ms on the same socket as the audio, and ack `SAak`, which the receiver sends back to
  the keepalive's source address and port. Both sides must run the same version.
- On Windows the sender mutes the default render endpoint (`IAudioEndpointVolume`) while
  acks are under 1.5 s old; see [Auto-mute](#auto-mute-of-the-macs-speakers). This runs on
  the keepalive thread, never in the capture callback. On Linux the sender only reports
  what Windows would do.
- The sender captures the default output device (WASAPI loopback on Windows). More than
  two channels: it sends only front left/right. Mono is copied to both channels.
- The capture callback never sends on the network itself (a stalled send would make it miss
  the audio deadline). It puts packets in a queue of 20 packets (100 ms), and a separate
  thread sends them. If the queue is full, the packet is dropped.
- The receiver waits until its buffer holds `--buffer-ms` of audio before playing. If the
  buffer runs dry, it plays silence and waits to fill up again (an "underrun"). If the buffer
  grows past the target plus max(target, 10 ms), it drops the oldest audio to get back to the
  target (a "trim").

## Build

```sh
cargo test
cargo build --release                                   # Linux: target/release/synergy-audio
rustup target add x86_64-pc-windows-gnu                 # once; needs mingw-w64-gcc
cargo build --release --target x86_64-pc-windows-gnu    # target/x86_64-pc-windows-gnu/release/synergy-audio.exe
```

## Run

On Linux (the machine that plays the audio):

```sh
synergy-audio receive                  # listens on UDP 0.0.0.0:24850, 20 ms buffer
synergy-audio receive --buffer-ms 40   # bigger buffer: fewer crackles, more latency
```

On Windows (the machine whose audio you want to hear):

```
synergy-audio.exe send 192.168.1.20         # port 24850 by default
synergy-audio.exe send 192.168.1.20:24850
synergy-audio.exe send 192.168.1.20 --log C:\path\synergy-audio.log   # unattended (autostart)
synergy-audio.exe unmute                    # undo auto-mute left by a killed sender
```

`synergy-audio --version` (or `-V`) prints the version.

`--log <file>` is for running without a terminal (the autostart task uses it). It writes
the status lines to that file instead of stderr, appending; when the file grows past 1 MiB
it is emptied and starts over (about a day of status lines). On errors (e.g. no audio
device yet right after login) it logs the error and retries every 10 s instead of exiting.
On Windows it also detaches from its console (`FreeConsole`), so the console window Task
Scheduler opens for it closes at once (a brief flash at most). Stop it with Task Manager or
`uninstall-autostart.ps1`, not Ctrl+C.

Both sides print status to stderr (or the `--log` file) every 5 s. The sender prints packets/s, which should be
about 200, plus running totals of packets dropped because the queue was full and of failed
sends, then whether the receiver answers and the mute state, e.g.
`send: 200 packets/s, queue-full dropped 0, send errors 0, receiver ok (ack 0.3 s ago), muted by us`
or `send: 0 packets/s, queue-full dropped 0, send errors 4, receiver not heard (last ack 12 s ago), not muted`.
The mute part is `muted by us`, `not muted`, `muted by you (left alone)`, `mute control
failed: ...`, or on Linux `no muting on this OS (Windows would be muted)`. Send errors are
normal while the receiver isn't running yet. The receiver prints packets/s, buffered ms, underruns, late/duplicate packets
dropped, and trims. If underruns keep rising, raise `--buffer-ms`.

## Dev scripts

Conveniences for the author's own setup (Linux desktop at 192.168.254.90, user `zpt`, repo
in `~/projects/synergy-audio`); edit the defaults for anything else.

On Linux, `./dev.sh` builds the Linux and Windows release binaries, then runs the receiver.
Arguments are passed to `receive`:

```sh
./dev.sh
./dev.sh --buffer-ms 40
```

If the user service is installed (`~/.config/systemd/user/synergy-audio.service` exists),
`./dev.sh` instead runs `./install-autostart.sh` with its arguments (only `--buffer-ms N`
is accepted then; without it the installed buffer is kept), which copies the new binary
and restarts the service, then follows the log with `journalctl --user -u synergy-audio -f
-o cat`. Ctrl+C stops only the log view.

`install-autostart.sh [--buffer-ms N]` runs `cargo build --release` (when `Cargo.toml` is
next to it; in an extracted release it uses the bundled `synergy-audio` instead, and
without either it stops with an error), copies the binary to `~/.local/bin/` (temp file + rename, so it works while
the old one runs), writes the user unit `~/.config/systemd/user/synergy-audio.service`
(`Wants=pipewire.service`, `After=pipewire.service pipewire-pulse.service`,
`ExecStart=%h/.local/bin/synergy-audio receive --buffer-ms N`, `Restart=on-failure`,
`RestartSec=3`, `WantedBy=default.target`; N defaults to the installed value, else 40),
stops receivers started by hand, opens UDP 24850 in ufw or firewalld if active and not open
yet (with sudo), then `daemon-reload`, `enable` and `restart`, and prints the status.
`uninstall-autostart.sh` disables and stops the service and removes the unit, keeping the
binary and the firewall rule (it prints how to remove them).

On Windows, `update.ps1` copies the Windows build from the desktop with `scp` into its own
folder and starts `synergy-audio.exe send <desktop>`. If the copy fails it stops; usually
the old sender is still running and holds the .exe, so close it first. First time only,
fetch the script itself (Windows PowerShell, in the folder you want to use):

```
scp zpt@192.168.254.90:projects/synergy-audio/update.ps1 .
```

Then each time:

```
powershell -ExecutionPolicy Bypass -File .\update.ps1
powershell -ExecutionPolicy Bypass -File .\update.ps1 -Desktop 192.168.1.20 -User me
```

If autostart is installed (scheduled task `synergy-audio`), `update.ps1` instead stops the
task and any running sender, copies the build into `$env:LOCALAPPDATA\synergy-audio\`,
restarts the task (on the old build if the copy failed) and shows the log's last lines.
`-Desktop` then only picks where to copy from; the target the task sends to is set by
`install-autostart.ps1 -Desktop`.

`install-autostart.ps1 [-Desktop host]` copies `synergy-audio.exe` from its own folder to
`$env:LOCALAPPDATA\synergy-audio\` and registers (or replaces) a Task Scheduler task
`synergy-audio` (removing the downloaded-file mark with `Unblock-File`): at logon of the current user (10 s delay), interactive (runs only while
logged on, in your session), action `synergy-audio.exe send <host> --log
$env:LOCALAPPDATA\synergy-audio\synergy-audio.log`, no time limit, runs on battery, restart
every minute on failure; then starts it. `uninstall-autostart.ps1` stops and removes the
task, stops any running `synergy-audio.exe` and runs `synergy-audio.exe unmute`, keeping
the folder. Fetch both once with
`scp` (setup step 18).

## Firewall

The receiver needs inbound UDP 24850. On the Linux machine, ufw is active (firewalld is
not). `install-autostart.sh` opens the port if it isn't open yet (ufw or firewalld); to
open it by hand:

```sh
sudo ufw allow 24850/udp
```

Windows only sends, so its firewall normally needs no change. If Windows asks about network
access the first time you run it, allow it on private networks.

## Releases and CI

GitHub Actions run on every push and pull request (`.github/workflows/ci.yml`): `cargo
test` and `cargo build --release` on Ubuntu 22.04 and Windows.

Pushing a tag `v*` runs `.github/workflows/release.yml`: it checks the tag matches the
`version` in `Cargo.toml` (and fails otherwise), builds Linux on Ubuntu 22.04 and Windows
with MSVC and a static C runtime (no VC++ redistributable needed), packages
`synergy-audio-<tag>-linux-x86_64.tar.gz` (folder `synergy-audio-<tag>/` with the binary,
`install-autostart.sh`, `uninstall-autostart.sh`, README, LICENSE) and
`synergy-audio-<tag>-windows-x86_64.zip` (files at the zip root: `synergy-audio.exe`,
`install-autostart.ps1`, `uninstall-autostart.ps1`, README, LICENSE; `update.ps1` is the
author's dev tool and is left out), writes `SHA256SUMS`, and creates the GitHub release with
generated notes.

### Making a release (maintainer)

1. Bump `version` in `Cargo.toml` (e.g. `0.2.0`), run `cargo build` so `Cargo.lock`
   follows, update the Status line above, commit and push.
2. Tag and push the tag:
   ```sh
   git tag v0.2.0 && git push origin v0.2.0
   ```
3. Watch the Actions tab; the release appears on the Releases page when it finishes. If
   the version guard fails (tag ≠ `Cargo.toml` version), delete the tag
   (`git tag -d v0.2.0 && git push origin :refs/tags/v0.2.0`), fix, and tag again.

## License

GPL-3.0-or-later, see [LICENSE](LICENSE). Copyright (c) 2026 ztancrell.
