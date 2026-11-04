# synergy-audio

Shares audio between two machines that already share keyboard and mouse with Synergy 3
(which carries no audio). One machine captures everything it plays and streams it over UDP
on the LAN; the other plays it on its default output, with a small jitter buffer for low
latency.

**Status: early, step 1.** Windows → Linux only, terminal only. No GUI, no Synergy
integration, no switching direction.

## How it works

- Format: uncompressed 16-bit (i16) stereo, 48 kHz. Both devices must run at 48 kHz (no
  resampling); otherwise it exits with an error that says how to change the rate.
- One UDP packet = 8-byte sequence number (u64, little-endian) + 5 ms of audio
  (240 frames = 960 bytes). Packets that arrive late or twice are dropped.
- The sender captures the default output device (WASAPI loopback on Windows). More than
  two channels: it sends only front left/right. Mono is copied to both channels.
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
about 200. The receiver prints packets/s, buffered ms, underruns, late/duplicate packets
dropped, and trims. If underruns keep rising, raise `--buffer-ms`.

## Firewall

The receiver needs inbound UDP 24850. On the Linux machine, ufw is active (firewalld is
not), so open the port:

```sh
sudo ufw allow 24850/udp
```

Windows only sends, so its firewall normally needs no change. If Windows asks about network
access the first time you run it, allow it on private networks.
