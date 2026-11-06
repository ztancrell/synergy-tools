# synergy-tools: audio

Shares audio between two machines that already share keyboard and mouse with Synergy 3
(which carries no audio). One machine captures everything it plays and streams it over UDP
on the LAN; the other plays it on its default output, with a small jitter buffer for low
latency. The stream is encrypted and authenticated with the partner secret from
`synergy-tools pair` ([Pairing and encryption](#pairing-and-encryption)).

**Status:** part of `synergy-tools` (`src/audio.rs`), command line only for now:
`synergy-tools audio send|receive|unmute`. Windows sender → Linux receiver. The tray starts
audio by itself in a later step (phase 1, Task 6).

## Download and install

Superseded: installers, autostart and releases come in phase 3; build from source
([Build](#build)). The old `synergy-audio` releases and install scripts are gone from this repo.

### macOS (sender)

Superseded: macOS is not supported by synergy-tools until phase 2; the released synergy-audio cannot pair with it.

## Author's setup

What this is built and tested on:

- **Main machine: CachyOS Linux desktop.** Wired Ethernet (`192.168.254.90`), headphones
  plugged in. Synergy 3 server: its keyboard and mouse control both machines. Runs
  `synergy-tools audio receive`.
- **Gaming machine: MacBook Pro 2019** (Intel i9, Radeon Pro 5500M 8 GB, 32 GB RAM, 1 TB)
  running **Windows 10 through Boot Camp** (with FlexTPM), on Wi-Fi. Synergy 3 client. Plays
  **Battlefield 6**. Runs `synergy-tools audio send`.

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
2. Open the audio port in the firewall (ufw):
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

5. On Linux, build (see [Build](#build) for the Windows cross-compile):
   ```sh
   cd ~/projects/synergy-tools && cargo build --release --target x86_64-pc-windows-gnu && cargo build --release
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
8. Pair the two machines once with `synergy-tools pair` (one pairing for audio and files; see
   [Pairing and encryption](#pairing-and-encryption)). Then start the receiver on Linux and
   the sender on Windows, pointing at the Linux machine's IP (or its partner name):
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

### Every update after that (dev scripts)

Superseded: the `synergy-audio` dev scripts (`dev.sh`, `update.ps1`) are gone; rebuild and copy
as in steps 5-7.

### Start the receiver automatically at Linux login (optional)

Superseded: autostart comes with phase 3's installers; the tray starts audio by itself (phase 1, Task 6).

### Start the sender automatically at Windows login (optional)

Superseded: autostart comes with phase 3's installers; the tray starts audio by itself (phase 1, Task 6).

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
  `%LOCALAPPDATA%\synergy-tools\muted-by-us`, while it has the speakers muted; the next
  start of the sender sees it and unmutes first, and `synergy-tools.exe audio unmute` does
  the same. Ctrl+C in the sender's terminal unmutes right away. If the speakers ever stay
  muted with no sender running, run `synergy-tools.exe audio unmute` or just unmute with
  the taskbar icon.

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
(`synergy-tools audio receive --buffer-ms 60`). Wi-Fi on the sending machine is the main cause; a wired
connection allows a smaller buffer and less delay.

## How it works

- Format: uncompressed 16-bit (i16) stereo, 48 kHz. Both devices must run at 48 kHz (no
  resampling); otherwise it exits with an error that says how to change the rate.
- Session: when the sender starts (and again whenever it has had no ack for 3 s, e.g. the
  receiver restarted) it does a Noise handshake, `Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s`
  (`snow` crate) with the pairing secret as the PSK: hello = `SAh1` + its 8-byte peer id
  (the partner id, cleartext, so the receiver can pick the secret) + Noise message 1; reply = `SAh2` +
  message 2. The receiver accepts a new handshake at any time but keeps the running session
  until a packet authenticates under the new one, so a replayed old hello changes nothing.
- After the handshake every packet, both ways, is `D` + 8-byte nonce (u64, little-endian) +
  ChaCha20-Poly1305 ciphertext + 16-byte tag. Audio: nonce = sequence number, payload =
  5 ms of audio (240 frames = 960 bytes), 985 bytes in all. Keepalive (sender, every
  500 ms, same socket as the audio): nonce with the top bit set. Ack (receiver, back to the
  keepalive's source address and port): its own counter. Both carry the 9-byte stop
  control as payload: 1 byte stopped (0 or 1) + 8-byte epoch (u64, little-endian); any
  other payload is rejected. Each side merges the other's into its own, last writer wins:
  the higher epoch wins, a tie goes to stopped. Start or Stop on either side (from the tray,
  phase 1 Task 6; the CLI never stops) bumps the epoch. While stopped the
  sender drops captured audio, the receiver discards audio and empties its buffer, and the
  sender unmutes. Per session and type the nonce must grow: late, duplicate and replayed packets
  are dropped (`late/dup dropped`). Anything that doesn't authenticate (unknown peer id,
  bad tag, wrong length, an old version's packet) is dropped and only counted (`rejected`);
  it never reaches playback or the mute logic. Both sides must run the same version.
- Superseded: up to v0.2.0 packets were plaintext and unauthenticated (audio = 8-byte
  sequence number + samples, keepalive `SAka`, ack `SAak`). The current format is the one above (also at the top of
  `src/audio.rs`).
- On Windows the sender mutes the default render endpoint (`IAudioEndpointVolume`) while
  acks are under 1.5 s old, the stream isn't stopped and auto-mute is on (always, from
  the CLI); it unmutes when any of those ends, and when `send` is told to quit; see [Auto-mute](#auto-mute-of-the-macs-speakers). This runs on
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

Superseded: `synergy-audio pair`/`unpair` and its pairing files (`receiver`, `senders/`) are
gone. Audio uses the partners from `synergy-tools pair` (one pairing for audio and files):
see [files.md, Pairing](files.md#pairing). Each partner has a 32-byte secret and an 8-byte
partner id derived from it, the same on both sides; the sender's hello carries that id.
`audio send <host>` picks the partner by name or host; `audio receive` accepts every partner
(it reads the partner list again when an unknown id says hello, at most once a second) but
plays one at a time: two senders running at once would take turns. Without a partner both exit
with a message that points at `synergy-tools pair`.

What it protects against: other devices on the LAN can't play audio through your receiver,
can't listen to the stream, can't fake acks to mute (or unmute) the sender's speakers, and
can't replay recorded packets. What it doesn't: anyone on the network still sees that
audio flows, when and how much, and can block or flood it; and anyone who can read the
partner list on either machine can impersonate it (`synergy-tools unpair` and pair again if
that happens). `synergy-tools` can't talk to the old `synergy-audio`.

## Build

See the root [README](../README.md#build). The program is `target/release/synergy-tools`
(Windows: `target/x86_64-pc-windows-gnu/release/synergy-tools.exe`).

## Run

Pair first ([Pairing and encryption](#pairing-and-encryption)). On Linux (the machine that
plays the audio):

```sh
synergy-tools audio receive                  # listens on UDP 0.0.0.0:24850, 20 ms buffer
synergy-tools audio receive --buffer-ms 40   # bigger buffer: fewer crackles, more latency
```

If UDP 24850 is busy it says so and names the likely holder (another `synergy-tools` or the
old `synergy-audio` receiver: `systemctl --user disable --now synergy-audio`).

On Windows (the machine whose audio you want to hear):

```
synergy-tools.exe audio send 192.168.1.20         # port 24850 by default
synergy-tools.exe audio send 192.168.1.20:24850
synergy-tools.exe audio send 192.168.1.20 --log C:\path\audio.log   # unattended
synergy-tools.exe audio unmute                    # undo auto-mute left by a killed sender
```

`--log <file>` is for running without a terminal. It writes the status lines to that file
instead of stderr, appending; when the file grows past 1 MiB it is emptied and starts over
(about a day of status lines). On errors (e.g. no audio device yet right after login) it
logs the error and retries every 10 s instead of exiting. Stop it with Task Manager, not
Ctrl+C.

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

Superseded: `dev.sh`, `update.ps1` and the install/uninstall scripts were removed with the
`synergy-audio/` folder; phase 3 adds installers for `synergy-tools`.

## Firewall

The receiver needs inbound UDP 24850. On the Linux machine, ufw is active (firewalld is
not); open it by hand:

```sh
sudo ufw allow 24850/udp
```

Windows only sends, so its firewall normally needs no change. If Windows asks about network
access the first time you run it, allow it on private networks.

## Releases and CI

Superseded: the `audio-ci.yml` and `audio-release.yml` workflows are removed; `ci.yml` builds
and tests `synergy-tools` (see the root [README](../README.md#ci-and-releases)). Phase 3 adds
one release workflow. Old `audio-v*` releases stay on the Releases page.

### Making a release (maintainer)

Superseded: per-tool releases end here; phase 3 adds one release workflow.

## License

GPL-3.0-or-later, see [LICENSE](../LICENSE). Copyright (c) 2026 ztancrell.
