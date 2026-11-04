# synergy-audio

Shares audio between two machines that already share keyboard and mouse with Synergy 3
(which carries no audio). One machine captures everything it plays and streams it over UDP
on the LAN; the other plays it on its default output, with a small jitter buffer for low
latency.

**Status: early, step 1.** Windows → Linux only, terminal only. No GUI, no Synergy
integration, no switching direction.

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
12. On Windows, stop any running sender (Ctrl+C), then fetch the newest build and start
    sending (it asks for your Linux password):
    ```
    powershell -ExecutionPolicy Bypass -File .\update.ps1
    ```

**Mute Windows to stop double audio.** The sender copies what Windows plays; it doesn't
redirect it, so you hear the sound from both the Mac's speakers and your headphones. Mute
Windows (the taskbar speaker icon): on the author's MacBook Pro, capture happens before the
volume control, so the speakers go silent and the headphones keep playing. If muting
silences your headphones too, your sound driver applies the volume before capture, so this
trick won't work on that machine.

**Volume.** For the same reason, the main Windows volume slider no longer changes what you
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
- One UDP packet = 8-byte sequence number (u64, little-endian) + 5 ms of audio
  (240 frames = 960 bytes). Packets that arrive late or twice are dropped.
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
```

Both sides print status to stderr every 5 s. The sender prints packets/s, which should be
about 200, plus running totals of packets dropped because the queue was full and of failed
sends, e.g. `send: 200 packets/s, queue-full dropped 0, send errors 0`. Send errors are
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

## Firewall

The receiver needs inbound UDP 24850. On the Linux machine, ufw is active (firewalld is
not), so open the port:

```sh
sudo ufw allow 24850/udp
```

Windows only sends, so its firewall normally needs no change. If Windows asks about network
access the first time you run it, allow it on private networks.
