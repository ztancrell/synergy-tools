# Audio (`synergy-tools audio`)

Shares audio between two machines that already share keyboard and mouse with Synergy 3
(which carries no audio). One machine captures everything it plays and streams it over UDP
on the LAN; the other plays it on its default output, with a small jitter buffer for low
latency. The stream is encrypted and authenticated: the two machines are paired once with an
8-digit code ([Pairing and encryption](#pairing-and-encryption)).

**Status: early.** Part of `synergy-tools` (formerly the standalone synergy-audio): Windows or macOS
sender → Linux receiver, from the tray ([In the tray](#in-the-tray)) or the terminal
(`synergy-tools audio send|receive|unmute`). One pairing covers audio and files
([Pairing](#pairing-and-encryption)). Not compatible with the old synergy-audio binaries
(different keys and pairing). No switching direction, no self-update yet.

## In the tray

Run `synergy-tools` (no arguments) on both PCs once they are paired, and audio streams by
itself: the Windows and macOS trays stream to the first online Linux partner, the Linux tray plays what
it receives. The role is fixed by platform (Linux receives; Windows and macOS send). If the partner
goes offline or another one comes online, the sender follows within about 6 s. Unpairing a
sender stops the Linux side playing it within about 5 s.

The tray menu's audio section:

- `Audio: streaming to <name>` (Windows, macOS) / `Audio: receiving from <name>` (Linux),
  `Audio: waiting for partner`, `Audio: stopped` or `Audio: error: <message>`; refreshed every
  second. A failed start (no audio device, ...) is retried every 10 s. If UDP 24850 is busy on
  Linux (an old synergy-audio receiver or a headless `audio receive` still running), the line
  shows that error and the tray retries the bind every 5 s, so audio starts by itself once the
  old receiver is gone; files keep working.
- `Stop audio` / `Start audio`: stops or restarts the stream on both sides; either PC can
  press it and the other follows within a second. Stopping unmutes the Windows speakers at
  once if we muted them. Stop is not saved: when both PCs restart, audio streams again.
  A PC that restarts alone adopts the other side's Stop (whoever changed it last wins), so
  restarting just one side does not undo a Stop.
- Windows only: `Auto-mute speakers` (on by default): mute the Windows speakers while the
  Linux side is playing ([Auto-mute](#auto-mute-of-the-speakers)). Saved across restarts
  (switched off = a file `auto-mute-off` in the synergy-tools config folder,
  `%APPDATA%\synergy-tools\`). Switching it off while muted unmutes at once. Quit stops the
  sender (waiting up to 1 s) and then unmutes if we muted, and so does the next start after a run that ended while muted (e.g.
  Windows shut down while streaming).

Don't run the headless `synergy-tools audio receive` while the Linux tray runs: both want UDP
24850. The headless commands also don't answer presence pings, so the other tray doesn't see
that PC as online and won't stream to it.

## Download and install

On Linux the receiver is the tray: `synergy-tools install` (see
[files.md "Install"](files.md#install)) starts it at desktop login, and the tray plays what the
partner streams. There is no systemd receiver service any more; `install` removes the old
`synergy-audio` one. On Windows the sender is the tray too: run `.\synergy-tools-windows-x86_64.exe install | Out-Host`
(same command, see [files.md "Install"](files.md#windows)); it first runs the old
`synergy-audio.exe unmute` if present, then removes the old `synergy-audio` scheduled task, and
starts the tray at login. On macOS the sender is the tray too: `./synergy-tools-macos-universal install` (see
[files.md "macOS"](files.md#macos)) makes the app `~/Applications/synergy-tools.app` and starts it at
login.

Headless (no desktop): `synergy-tools audio receive` is not installed by `install`. Run it
yourself, for example from your own systemd user unit, and never together with the tray (both
want UDP 24850).

Superseded: the systemd receiver service from the old synergy-audio installer; the tray replaces
it (src/install.rs).

### macOS (sender)

macOS 14.6 or later, universal binary (see the README). Set the output device to 48 kHz
(Audio MIDI Setup > the output device > Format). Install it as in
[files.md "macOS"](files.md#macos). It streams by itself to the first online Linux partner;
Stop/Start works as on Windows. There is no auto-mute on macOS: mute or lower the Mac's
speakers yourself.

The first run asks for **System Audio Recording** permission, naming synergy-tools (the app
started at login or by `install`; Terminal if you run the bare binary from Terminal): allow it
(System Settings > Privacy & Security > Screen & System Audio Recording), then Quit and open the
app again. After an update macOS may ask again. Without it Linux gets silence while the Mac says
streaming.

Superseded: running the tray from Terminal, so the prompt named Terminal; `install` replaces it
(src/install.rs).

## Author's setup

What this is built and tested on:

- **Main machine: CachyOS Linux desktop.** Wired Ethernet (`192.168.254.90`), headphones
  plugged in. Synergy 3 server: its keyboard and mouse control both machines. Runs the
  `synergy-tools` tray as the receiver (set up by `synergy-tools install`).
- **Gaming machine: MacBook Pro 2019** (Intel i9, Radeon Pro 5500M 8 GB, 32 GB RAM, 1 TB)
  running **Windows 10 through Boot Camp** (with FlexTPM), on Wi-Fi. Synergy 3 client. Plays
  **Battlefield 6**. Runs the `synergy-tools` tray as the sender, started at login by
  `synergy-tools install`; it streams by itself. (Superseded: `synergy-tools audio send` by hand.)

```
keyboard + mouse:  CachyOS desktop --(Synergy 3)--> MacBook Pro (Windows 10)
game audio:        MacBook Pro (Windows 10) --(synergy-tools audio, UDP 24850)--> CachyOS desktop --> headphones
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
2. Open the audio port in the firewall (ufw). By hand:
   ```sh
   sudo ufw allow 24850/udp
   ```
   The tray app also needs inbound TCP 24851 (pairing, files, and the presence pings that
   decide when to stream): see [files.md](files.md#pairing) and
   [Troubleshooting](files.md#troubleshooting).
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
   cd ~/projects/synergy-tools && cargo build --release
   ```
6. On Windows, open PowerShell and go to your Downloads folder (not `C:\Windows\system32`,
   where an admin PowerShell starts):
   ```
   cd $HOME\Downloads
   ```
7. Copy the program from Linux. The command is **`scp`**, not `cp` (in PowerShell `cp` is a
   local copy and fails with "Cannot find drive"):
   ```
   scp zpt@192.168.254.90:projects/synergy-tools/target/x86_64-pc-windows-gnu/release/synergy-tools.exe .
   ```
   The first time, it asks whether to trust the host key: type `yes`. Then enter your Linux
   password.
8. Pair the two machines (once; see [Pairing and encryption](#pairing-and-encryption)).
   On Linux, this shows a code like `1234-5678` and waits up to 2 minutes:
   ```sh
   target/release/synergy-tools pair
   ```
   On Windows, with that code:
   ```
   .\synergy-tools.exe pair 192.168.254.90 1234-5678
   ```
   Both sides say `Paired`. A wrong code fails on both sides; run `pair` on Linux again for
   a new code. One pairing also covers file transfer. Then start the receiver on Linux and the sender on Windows, pointing at the
   Linux machine's IP:
   ```sh
   target/release/synergy-tools audio receive
   ```
   ```
   .\synergy-tools.exe audio send 192.168.254.90
   ```
   If Windows shows "Windows protected your PC" (the program is unsigned), click
   **More info → Run anyway**. If either side exits with a 48 kHz error, set that sound
   device to 48000 Hz as the message says.
9. Play something on Windows and listen on Linux. For a clean test, play one continuous song
   for about 30 seconds and read both sides' status lines (see [Run](#run)). Packets/s drops
   to 0 whenever nothing is playing; that is normal.

### Every update after that, autostart

Linux, release download: `chmod +x synergy-tools-linux-x86_64 && ./synergy-tools-linux-x86_64 install`.
Linux, built from source: `cargo build --release && target/release/synergy-tools install`. Then Quit the
tray and start it again (`~/.local/bin/synergy-tools`; `~/.local/bin` is on PATH on most desktops).
Windows: copy the new `synergy-tools.exe` over as in steps 5-8 (or download
`synergy-tools-windows-x86_64.exe`), run its `install | Out-Host` in that folder, then Quit the tray and
start it again (`& "$env:LOCALAPPDATA\Programs\synergy-tools\synergy-tools.exe"`).
Superseded: the update line that ran the release asset name after a source build.

Superseded: dev scripts, Linux and Windows autostart and the update script are gone; the
tray starts audio by itself and `synergy-tools install` (src/install.rs) sets up Linux and Windows.

### Auto-mute of the speakers

The sender copies what Windows plays; it doesn't redirect it, so without muting you'd hear
the sound from both the Windows speakers and your headphones. **The sender now mutes Windows
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
- **If the sender is killed** (Task Manager, `Stop-Process`, a crash), it can't unmute. It leaves a marker file,
  `%LOCALAPPDATA%\synergy-tools\muted-by-us`, while it has the speakers muted; the next
  start of the sender sees it and unmutes first, and `synergy-tools.exe audio unmute`
  does the same. Ctrl+C
  or closing the sender's window unmutes right away. If the speakers ever stay muted with
  no sender running, run `synergy-tools.exe audio unmute` or just unmute with the taskbar icon.

This works because on the author's MacBook Pro capture happens before the volume control:
muted speakers go silent and the headphones keep playing. If muting silences your
headphones too, your sound driver applies the volume before capture, and auto-mute will
cut your stream while the receiver runs; you'd see `muted by us` in the status line and
hear nothing. Switch it off with the tray's `Auto-mute speakers`
([In the tray](#in-the-tray)); the terminal `audio send` always auto-mutes.

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
(`synergy-tools audio receive --buffer-ms 60`). Wi-Fi on the sending machine is the main cause; a wired
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
  `src/audio.rs`). Superseded: before the stop control, keepalive and ack payloads were empty.
- On Windows the sender mutes the default render endpoint (`IAudioEndpointVolume`) while
  acks are under 1.5 s old, the stream isn't stopped and auto-mute is on (always on in the
  CLI); it unmutes when any of that ends, and when `send` is told to quit; see [Auto-mute](#auto-mute-of-the-speakers). This runs on
  the keepalive thread, never in the capture callback. On Linux and macOS the sender only
  reports what Windows would do.
- The sender captures the default output device (WASAPI loopback on Windows, a CoreAudio process tap on macOS 14.6+). More than
  two channels: it sends only front left/right. Mono is copied to both channels.
- The capture callback never sends on the network itself (a stalled send would make it miss
  the audio deadline). It puts packets in a queue of 20 packets (100 ms), and a separate
  thread sends them. If the queue is full, the packet is dropped.
- The receiver waits until its buffer holds `--buffer-ms` of audio before playing. If the
  buffer runs dry, it plays silence and waits to fill up again (an "underrun"). If the buffer
  grows past the target plus max(target, 10 ms), it drops the oldest audio to get back to the
  target (a "trim").

## Pairing and encryption

Pairing is shared with file transfer: once per pair of PCs, terminal only:

```sh
synergy-tools pair                             # on one PC: prints a code like 1234-5678
synergy-tools pair 192.168.1.20 1234-5678      # on the other, within 2 minutes
synergy-tools unpair <name>                    # forget a partner
```

Details (SPAKE2 with the code, the partner list, files and ports) are in
[files.md](files.md#pairing). Pairing gives both sides a 32-byte secret and an 8-byte id
(the audio peer id). The receiver accepts every partner in the list (`partners` file in
`~/.config/synergy-tools/` on Linux, `%APPDATA%\synergy-tools\` on Windows, `~/Library/Application Support/synergy-tools/` on macOS) and plays one at
a time: two senders running at once would take turns. `audio send <target>` picks the partner
by name, `name/os` or host and connects to that partner's stored address (UDP port 24850,
or the `:port` you add to the target). `audio send` and `audio receive` without a pairing exit with a
message that points at `synergy-tools pair`.

Superseded: the audio-only UDP pairing (`synergy-audio pair`, `senders/<id>` and `receiver`
files) is gone; see `src/pair.rs` and `src/config.rs`.

What it protects against: other devices on the LAN can't play audio through your receiver,
can't listen to the stream, can't fake acks to mute (or unmute) the sender's speakers, and
can't replay recorded packets. What it doesn't: anyone on the network still sees that
audio flows, when and how much, and can block or flood it; and anyone who can read the
pairing files on either machine can impersonate it (unpair and pair again if that
happens). Versions with encryption can't talk to v0.2.0 or older: update both machines.

## Build

```sh
cargo test
cargo build --release                                   # Linux: target/release/synergy-tools
rustup target add x86_64-pc-windows-gnu                 # once; needs mingw-w64-gcc
cargo build --release --target x86_64-pc-windows-gnu    # target/x86_64-pc-windows-gnu/release/synergy-tools.exe
```

Linux needs `libgtk-3-dev libayatana-appindicator3-dev libasound2-dev` (Arch `gtk3
libayatana-appindicator alsa-lib`).

## Run

Pair first ([Pairing and encryption](#pairing-and-encryption)). On Linux (the machine that
plays the audio):

```sh
synergy-tools audio receive                  # listens on UDP 0.0.0.0:24850, 20 ms buffer
synergy-tools audio receive --buffer-ms 40   # bigger buffer: fewer crackles, more latency
```

On Windows (the machine whose audio you want to hear):

```
synergy-tools.exe audio send <partner>[:port]     # partner: name, name/os or host
synergy-tools.exe audio send 192.168.1.20         # port 24850 by default
synergy-tools.exe audio send 192.168.1.20:24850
synergy-tools.exe audio send 192.168.1.20 --log C:\path\audio.log   # unattended, retries on errors
synergy-tools.exe audio unmute                    # undo auto-mute left by a killed sender
```

On macOS, `./synergy-tools audio send <partner>` works the same (no `--log` console notes, no `unmute`).

After install the program is `& "$env:LOCALAPPDATA\Programs\synergy-tools\synergy-tools.exe" … | Out-Host`
on Windows and `~/Applications/synergy-tools.app/Contents/MacOS/synergy-tools …` on macOS; on Linux
`~/.local/bin/synergy-tools` (usually on PATH). `uninstall` on Windows also unmutes the speakers if
the tray had muted them.

`synergy-tools --version` prints the version.

`--log <file>` is for running without a terminal (for unattended runs). It writes
the status lines to that file instead of stderr, appending; when the file grows past 1 MiB
it is emptied and starts over (about a day of status lines). On errors (e.g. no audio
device yet right after login) it logs the error and retries every 10 s instead of exiting.
The Windows binary has no console of its own, so started without a terminal it shows no
window. Stop it with Task Manager, not Ctrl+C.

Both sides print status to stderr (or the `--log` file) every 5 s. The sender prints packets/s, which should be
about 200, plus running totals of packets dropped because the queue was full and of failed
sends and of received packets that failed to authenticate (`rejected`), then whether the
receiver answers and the mute state, e.g.
`send: 200 packets/s, queue-full dropped 0, send errors 0, rejected 0, receiver ok (ack 0.3 s ago), muted by us`
or `send: 0 packets/s, queue-full dropped 0, send errors 4, rejected 0, receiver not heard (last ack 12 s ago), not muted`.
Until the handshake is done (`send: encrypted session with the receiver started`) it sends
no audio, so packets/s stays 0.
The mute part is `muted by us`, `not muted`, `muted by you (left alone)`, `mute control
failed: ...`, or on Linux and macOS `no muting on this OS (Windows would be muted)`. Send errors are
normal while the receiver isn't running yet. The receiver prints packets/s, buffered ms, underruns, late/duplicate/replayed packets
dropped, packets rejected (failed to authenticate), and trims. If underruns keep rising, raise `--buffer-ms`.

## Dev scripts

Superseded: `dev.sh` and `update.ps1` are gone; build with `cargo build --release`.

## Firewall

The receiver needs inbound UDP 24850. In the tray it also needs inbound TCP 24851: the
Windows and macOS trays only stream to a Linux partner that answers its presence ping on that port
(pairing and files use it too; see [files.md](files.md#pairing) and
[Troubleshooting](files.md#troubleshooting)). On the Linux machine, ufw is active (firewalld
is not). To open both:

```sh
sudo ufw allow 24850/udp
sudo ufw allow 24851/tcp
```

Windows and macOS only send, so Windows needs no firewall change for audio (`install` adds the
`synergy-tools (TCP 24851)` rule for file transfer and presence; if it asks about network access
the first time you run it, allow it on private networks). The Mac needs inbound
TCP 24851 for the presence ping; see [files.md](files.md#macos).

## Releases and CI

One `v*` tag publishes the Linux, Windows and macOS binaries to a GitHub Release; see the README "CI and releases".

Superseded: per-tool tags `audio-v*` / `dragndrop-v*` and their workflows; one `v*` tag now (`.github/workflows/release.yml`).

### Making a release (maintainer)

Bump `version` in `Cargo.toml`, commit, `git tag v<version> && git push origin v<version>`; `release.yml` does the rest (README "CI and releases").

Superseded: the per-tool release steps that stood here; see Releases and CI above.

## License

GPL-3.0-or-later, see [LICENSE](../LICENSE). Copyright (c) 2026 ztancrell.
