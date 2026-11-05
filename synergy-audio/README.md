# synergy-audio

[![CI](https://github.com/ztancrell/synergy-tools/actions/workflows/audio-ci.yml/badge.svg)](https://github.com/ztancrell/synergy-tools/actions/workflows/audio-ci.yml)

Shares audio between two machines that already share keyboard and mouse with Synergy 3
(which carries no audio). One machine captures everything it plays and streams it over UDP
on the LAN; the other plays it on its default output, with a small jitter buffer for low
latency. The stream is encrypted and authenticated: the two machines are paired once with an
8-digit code ([Pairing and encryption](#pairing-and-encryption)).

**Status: early (v0.2.0).** Windows or macOS sender → Linux receiver, terminal only (plus
optional autostart: a systemd user service for the Linux receiver, a scheduled task for the
Windows sender; none for macOS yet). The macOS sender is built by CI but not in a tagged
release yet. Encryption and pairing are newer than v0.2.0 and incompatible with it: update both
machines together.
Prebuilt downloads are the `audio-vX.Y.Z` releases on the
[synergy-tools Releases page](https://github.com/ztancrell/synergy-tools/releases) (none published yet).
No GUI, no Synergy integration, no switching direction, no self-update yet.

## Download and install

You need a Windows PC or a Mac (the one whose audio you want to hear) and a Linux PC with PipeWire
(the one with your headphones) on the same LAN. Both sound devices must run at 48 kHz.
From the [synergy-tools Releases page](https://github.com/ztancrell/synergy-tools/releases), download
the two archives of the newest `audio-vX.Y.Z` release (the `dragndrop-v*` releases are the
other tool):

- `synergy-audio-vX.Y.Z-linux-x86_64.tar.gz` for the Linux PC (built on Ubuntu 22.04, runs
  on most current distros).
- `synergy-audio-vX.Y.Z-windows-x86_64.zip` for the Windows PC (64-bit Windows 10/11).
- `synergy-audio-vX.Y.Z-macos-universal.tar.gz` instead, if the sender is a Mac running macOS
  (see [macOS (sender)](#macos-sender) below).

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
`192.168.1.20`). If no sender is paired yet, it then shows a pairing code like `1234-5678`
and waits up to 2 minutes for the sender: leave it waiting and do the Windows (or macOS)
part now.

**Windows (sender):** right-click the zip → **Extract All**, open the extracted folder,
click in the address bar, type `powershell` and press Enter. Then, with your Linux PC's
address:

```
powershell -ExecutionPolicy Bypass -File .\install-autostart.ps1 -Desktop 192.168.1.20
```

Always pass `-Desktop`: without it the sender goes to the author's address. If this PC isn't
paired yet, it asks for the pairing code the Linux PC shows; type it (with or without the
dash). It copies the
program to `%LOCALAPPDATA%\synergy-audio` and starts it at every login, hidden (details:
steps 19-23 below). If Windows shows "Windows protected your PC" when you run
`synergy-audio.exe` yourself (the program is unsigned), click **More info → Run anyway**.
Windows only sends, so its firewall needs no change.

Play something on Windows: it should come out of the Linux PC, and the Windows speakers
mute while it does ([Auto-mute](#auto-mute-of-the-macs-speakers)). To check, see steps
14 and 20 below. To remove: `./uninstall-autostart.sh` on Linux,
`powershell -ExecutionPolicy Bypass -File .\uninstall-autostart.ps1` on Windows.
To update: download the new release and run the same install commands again (the pairing
is kept, so no new code is asked). Coming from v0.2.0 or older (no encryption): update both
machines, the old and new versions can't talk to each other.

Optional: check the downloads against `SHA256SUMS` from the same release page:
`sha256sum -c --ignore-missing SHA256SUMS` on Linux, `Get-FileHash <file>` on Windows
(compare by eye).

To build from source instead, follow [Setup, step by step](#setup-step-by-step).

### macOS (sender)

Needs macOS 14.6 or newer (Apple Silicon or Intel: the binary is universal). The Mac's
output device must run at 48 kHz (Audio MIDI Setup). Set up the Linux receiver as above,
then in Terminal:

```sh
tar xf synergy-audio-vX.Y.Z-macos-universal.tar.gz
cd synergy-audio-vX.Y.Z
xattr -d com.apple.quarantine ./synergy-audio
./synergy-audio pair 192.168.1.20 1234-5678   # once, with the code the Linux PC shows
./synergy-audio send 192.168.1.20
```

The Linux PC shows the code while `install-autostart.sh` waits for a sender. If it is
already installed and paired with another sender, pair this one too: on Linux run
`systemctl --user stop synergy-audio && ~/.local/bin/synergy-audio pair`, then the `pair`
line above on the Mac, then `systemctl --user start synergy-audio`.

The program is unsigned, so Gatekeeper blocks it without the `xattr` line. Alternatively,
run it once, then allow it in System Settings → Privacy & Security → **Open Anyway**. On
first run macOS asks for **System Audio Recording** permission (the prompt may name
Terminal rather than synergy-audio); allow it, then run the command again if no audio
arrives. Auto-mute and autostart are Windows/Linux only for now: on the Mac, mute or lower
the speakers yourself, and start the sender by hand. Stop it with Ctrl+C.

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

5. On Linux, build:
   ```sh
   cd ~/projects/synergy-tools/synergy-audio && cargo build --release
   ```
6. On Windows, open PowerShell and go to your Downloads folder (not `C:\Windows\system32`,
   where an admin PowerShell starts):
   ```
   cd $HOME\Downloads
   ```
7. Copy the program from Linux. The command is **`scp`**, not `cp` (in PowerShell `cp` is a
   local copy and fails with "Cannot find drive"):
   ```
   scp zpt@192.168.254.90:projects/synergy-tools/synergy-audio/target/x86_64-pc-windows-gnu/release/synergy-audio.exe .
   ```
   The first time, it asks whether to trust the host key: type `yes`. Then enter your Linux
   password.
8. Pair the two machines (once; see [Pairing and encryption](#pairing-and-encryption)).
   On Linux, this shows a code like `1234-5678` and waits up to 2 minutes:
   ```sh
   target/release/synergy-audio pair
   ```
   On Windows, with that code:
   ```
   .\synergy-audio.exe pair 192.168.254.90 1234-5678
   ```
   Both sides say `Paired`. A wrong code fails on both sides; run `pair` on Linux again for
   a new code. Then start the receiver on Linux and the sender on Windows, pointing at the
   Linux machine's IP:
   ```sh
   target/release/synergy-audio receive
   ```
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
    scp zpt@192.168.254.90:projects/synergy-tools/synergy-audio/update.ps1 .
    ```
11. On Linux, stop any running receiver (Ctrl+C), then build both versions and start the
    receiver (40 ms buffer suits the Mac's Wi-Fi better than the 20 ms default):
    ```sh
    cd ~/projects/synergy-tools/synergy-audio && ./dev.sh --buffer-ms 40
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
    cd ~/projects/synergy-tools/synergy-audio && ./install-autostart.sh
    ```
    It copies the program to `~/.local/bin/synergy-audio`, writes
    `~/.config/systemd/user/synergy-audio.service` (runs `synergy-audio receive --buffer-ms
    40`), enables and starts it. A receiver you started by hand in a terminal holds the
    audio port, so the script stops it (it says so). If ufw (or firewalld) is active and
    UDP 24850 isn't open yet, it opens it and **asks for your sudo password** for that only;
    if the port is already open it doesn't ask. If no sender is paired yet (step 8 not
    done), it runs `synergy-audio pair` before starting the service and waits for the
    sender's `pair` command. For another buffer:
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
    Once the sender runs you should see `receive: encrypted session with sender ... at ...`
    and the 5-second status lines (packets/s above 0 while something plays).
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
    scp zpt@192.168.254.90:projects/synergy-tools/synergy-audio/install-autostart.ps1 .
    scp zpt@192.168.254.90:projects/synergy-tools/synergy-audio/uninstall-autostart.ps1 .
    ```
19. Close any sender you started by hand (Ctrl+C in its window), then install. A normal
    (not administrator) PowerShell is enough:
    ```
    powershell -ExecutionPolicy Bypass -File .\install-autostart.ps1
    ```
    It copies `synergy-audio.exe` to `C:\Users\bootcamp\AppData\Local\synergy-audio\`
    (`$env:LOCALAPPDATA\synergy-audio`), registers a Task Scheduler task named
    `synergy-audio` that runs it 10 s after you log in (only while you're logged on, so it
    can capture your audio), and starts it right away. If Windows isn't paired yet (step 8
    not done), it first asks for the code that `synergy-audio pair` shows on Linux. To send
    to another IP:
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
  ack. Both are encrypted and authenticated, so nobody else on the LAN can fake acks to
  mute your speakers. While the last ack is less than 1.5 s old, the sender keeps the Windows default
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
- Session: when the sender starts (and again whenever it has had no ack for 3 s, e.g. the
  receiver restarted) it does a Noise handshake, `Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s`
  (`snow` crate) with the pairing secret as the PSK: hello = `SAh1` + its 8-byte peer id
  (cleartext, so the receiver can pick the secret) + Noise message 1; reply = `SAh2` +
  message 2. The receiver accepts a new handshake at any time but keeps the running session
  until a packet authenticates under the new one, so a replayed old hello changes nothing.
- After the handshake every packet, both ways, is `D` + 8-byte nonce (u64, little-endian) +
  ChaCha20-Poly1305 ciphertext + 16-byte tag. Audio: nonce = sequence number, payload =
  5 ms of audio (240 frames = 960 bytes), 985 bytes in all. Keepalive (sender, every
  500 ms, same socket as the audio): nonce with the top bit set. Ack (receiver, back to the
  keepalive's source address and port): its own counter. Keepalive and ack both carry the
  9-byte stop control: 1 byte stopped (0 or 1) + 8-byte epoch (u64, little-endian); any other
  payload is rejected. Each side merges the other's into its own (last writer wins: the
  higher epoch wins, and on a tie stopped wins) and answers with its own; every stop or
  resume adds 1 to the epoch. While stopped the sender drops captured audio, the receiver
  discards any that arrives and empties its buffer, and the sender doesn't mute. The CLI never
  stops (it has no stop command); stop is not saved across restarts. Per session and type the nonce must grow: late, duplicate and replayed packets
  are dropped (`late/dup dropped`). Anything that doesn't authenticate (unknown peer id,
  bad tag, wrong length, an old version's packet) is dropped and only counted (`rejected`);
  it never reaches playback or the mute logic. Both sides must run the same version.
- Superseded: up to v0.2.0 packets were plaintext and unauthenticated (audio = 8-byte
  sequence number + samples, keepalive `SAka`, ack `SAak`). The current format is the one above (also at the top of
  `src/main.rs`). Superseded: before the stop control, keepalive and ack payloads were empty.
- On Windows the sender mutes the default render endpoint (`IAudioEndpointVolume`) while
  acks are under 1.5 s old, the stream isn't stopped and auto-mute is on (always on in the
  CLI); it unmutes when any of that ends, and when `send` is told to quit; see [Auto-mute](#auto-mute-of-the-macs-speakers). This runs on
  the keepalive thread, never in the capture callback. On macOS and Linux the sender only
  reports what Windows would do.
- The sender captures the default output device (WASAPI loopback on Windows, CoreAudio
  loopback on macOS 14.6+). More than
  two channels: it sends only front left/right. Mono is copied to both channels.
- The capture callback never sends on the network itself (a stalled send would make it miss
  the audio deadline). It puts packets in a queue of 20 packets (100 ms), and a separate
  thread sends them. If the queue is full, the packet is dropped.
- The receiver waits until its buffer holds `--buffer-ms` of audio before playing. If the
  buffer runs dry, it plays silence and waits to fill up again (an "underrun"). If the buffer
  grows past the target plus max(target, 10 ms), it drops the oldest audio to get back to the
  target (a "trim").

## Pairing and encryption

Pairing, once per sender/receiver pair, terminal only:

```sh
synergy-audio pair                             # on the receiver: prints a code like 1234-5678
synergy-audio pair 192.168.1.20 1234-5678      # on the sender, within 2 minutes
synergy-audio unpair                           # deletes all pairing files on this machine
```

`pair` on the receiver needs UDP 24850, so stop the receiver first (`systemctl --user stop
synergy-audio`, or Ctrl+C); it says so if the port is busy. It takes exactly one attempt:
with a wrong code both sides report `wrong code` and exit; run it again for a new code. The
code never crosses the network: the two sides run SPAKE2 (`spake2` crate, Ed25519) with it
and confirm the result both ways, so someone listening can't guess it offline, and someone
interfering gets one guess in 100 million. Pairing gives both sides a 32-byte secret, plus
an 8-byte id for the sender. The receiver can have several paired senders (one file each;
restart the receiver after pairing a new one), but plays one at a time: two senders running
at once would take turns. The sender keeps one receiver; pairing again replaces it. `send`
and `receive` without a pairing exit with a message that points at `synergy-audio pair`.

Pairing files (unix: mode 0600, readable only by you):

- Linux: `$XDG_CONFIG_HOME/synergy-audio/` or `~/.config/synergy-audio/`
- macOS: `~/Library/Application Support/synergy-audio/`
- Windows: `%APPDATA%\synergy-audio\` (the program and log stay in `%LOCALAPPDATA%`)

with `receiver` on a sender (`<host> <peer id> <secret>`, hex) and `senders/<peer id>` on a
receiver (the secret, hex).

What it protects against: other devices on the LAN can't play audio through your receiver,
can't listen to the stream, can't fake acks to mute (or unmute) the sender's speakers, and
can't replay recorded packets. What it doesn't: anyone on the network still sees that
audio flows, when and how much, and can block or flood it; and anyone who can read the
pairing files on either machine can impersonate it (unpair and pair again if that
happens). Versions with encryption can't talk to v0.2.0 or older: update both machines.

## Build

```sh
cargo test
cargo build --release                                   # Linux: target/release/synergy-audio
rustup target add x86_64-pc-windows-gnu                 # once; needs mingw-w64-gcc
cargo build --release --target x86_64-pc-windows-gnu    # target/x86_64-pc-windows-gnu/release/synergy-audio.exe
```

## Run

Pair first ([Pairing and encryption](#pairing-and-encryption)). On Linux (the machine that
plays the audio):

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
sends and of received packets that failed to authenticate (`rejected`), then whether the
receiver answers and the mute state, e.g.
`send: 200 packets/s, queue-full dropped 0, send errors 0, rejected 0, receiver ok (ack 0.3 s ago), muted by us`
or `send: 0 packets/s, queue-full dropped 0, send errors 4, rejected 0, receiver not heard (last ack 12 s ago), not muted`.
Until the handshake is done (`send: encrypted session with the receiver started`) it sends
no audio, so packets/s stays 0.
The mute part is `muted by us`, `not muted`, `muted by you (left alone)`, `mute control
failed: ...`, or on macOS and Linux `no muting on this OS (Windows would be muted)`. Send errors are
normal while the receiver isn't running yet. The receiver prints packets/s, buffered ms, underruns, late/duplicate/replayed packets
dropped, packets rejected (failed to authenticate), and trims. If underruns keep rising, raise `--buffer-ms`.

## Dev scripts

Conveniences for the author's own setup (Linux desktop at 192.168.254.90, user `zpt`, repo
in `~/projects/synergy-tools/synergy-audio`); edit the defaults for anything else.

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
yet (with sudo), runs `synergy-audio pair` if `${XDG_CONFIG_HOME:-~/.config}/synergy-audio/senders/`
is empty (stopping the service first), then `daemon-reload`, `enable` and `restart`, and prints the status.
`uninstall-autostart.sh` disables and stops the service and removes the unit, keeping the
binary and the firewall rule (it prints how to remove them).

On Windows, `update.ps1` copies the Windows build from the desktop with `scp` into its own
folder and starts `synergy-audio.exe send <desktop>`. If the copy fails it stops; usually
the old sender is still running and holds the .exe, so close it first. First time only,
fetch the script itself (Windows PowerShell, in the folder you want to use):

```
scp zpt@192.168.254.90:projects/synergy-tools/synergy-audio/update.ps1 .
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
every minute on failure; then starts it. Before that, if `%APPDATA%\synergy-audio\receiver`
doesn't exist, it asks for the pairing code and runs `synergy-audio.exe pair <host> <code>`
(stopping if that fails). `uninstall-autostart.ps1` stops and removes the
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

This tool lives in the `synergy-audio/` folder of the
[synergy-tools](https://github.com/ztancrell/synergy-tools) monorepo; its workflows are in the
repo root's `.github/workflows/`. GitHub Actions run on pushes to `main` and on pull requests that
touch `synergy-audio/` or the workflow itself (`.github/workflows/audio-ci.yml`), from the
`synergy-audio/` folder: `cargo
test` and `cargo build --release` on Ubuntu 22.04, Windows and macOS (macOS also builds
`x86_64-apple-darwin` and uploads it as the `macos-x86_64` workflow artifact, fetchable
with `gh run download <run-id> -n macos-x86_64`).

Pushing a tag `audio-v*` runs `.github/workflows/audio-release.yml`: it checks the tag (minus
`audio-v`) matches the
`version` in `Cargo.toml` (and fails otherwise), builds Linux on Ubuntu 22.04, Windows
with MSVC and a static C runtime (no VC++ redistributable needed), and macOS for
`aarch64-apple-darwin` and `x86_64-apple-darwin` joined with `lipo` into one universal
binary, packages
`synergy-audio-vX.Y.Z-linux-x86_64.tar.gz` (folder `synergy-audio-vX.Y.Z/` with the binary,
`install-autostart.sh`, `uninstall-autostart.sh`, README, LICENSE) and
`synergy-audio-vX.Y.Z-windows-x86_64.zip` (files at the zip root: `synergy-audio.exe`,
`install-autostart.ps1`, `uninstall-autostart.ps1`, README, LICENSE; `update.ps1` is the
author's dev tool and is left out) and `synergy-audio-vX.Y.Z-macos-universal.tar.gz` (folder
`synergy-audio-vX.Y.Z/` with the binary, README, LICENSE), writes `SHA256SUMS`, and creates the GitHub release
`audio-vX.Y.Z` with notes generated since the previous `audio-v*` release.

### Making a release (maintainer)

1. Bump `version` in `synergy-audio/Cargo.toml` (e.g. `0.2.0`), run `cargo build` so `Cargo.lock`
   follows, update the Status line above, commit and push.
2. Tag and push the tag:
   ```sh
   git tag audio-v0.2.0 && git push origin audio-v0.2.0
   ```
3. Watch the Actions tab; the release appears on the Releases page when it finishes. If
   the version guard fails (tag ≠ `Cargo.toml` version), delete the tag
   (`git tag -d audio-v0.2.0 && git push origin :refs/tags/audio-v0.2.0`), fix, and tag again.

## License

GPL-3.0-or-later, see [LICENSE](LICENSE). Copyright (c) 2026 ztancrell.
