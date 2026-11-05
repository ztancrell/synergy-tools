# synergy-tools

Small tools that fill gaps in [Synergy 3](https://symless.com/synergy), which shares keyboard
and mouse between computers but not audio or file drag-and-drop.

| Tool | What it does | Platforms |
|------|--------------|-----------|
| [audio](docs/audio.md) (`synergy-tools audio`) | Streams one machine's audio to another over the LAN (e.g. Windows game audio to Linux headphones). | Windows sender → Linux receiver |
| [files](docs/files.md) (`synergy-tools`) | Sends files between the two machines: drop window, right-click "Send to", tray. | Windows ↔ Linux |

Run `synergy-tools` with no arguments for the tray (one icon for both tools). Its menu:

```
Online: <names> | No partner online | Not paired
─────
Audio: streaming to <name> | receiving from <name> | waiting for partner | stopped | error: …
Stop audio / Start audio
Auto-mute speakers        (Windows only, a check mark)
─────
Hide drop window / Show drop window
─────
Pair…
Quit
```

Dropped files go to the online partner (a picker when several are online). Audio streams by
itself from Windows to the online Linux partner ([docs/audio.md](docs/audio.md#in-the-tray)).

Both pair the two machines once with an 8-digit code, and their traffic is encrypted and
authenticated. Usage: [docs/files.md](docs/files.md) and [docs/audio.md](docs/audio.md).

## Pairing

One code pairs two PCs: run `synergy-tools pair` on one and `synergy-tools pair <host> <code>`
on the other. The pairing covers audio and files alike. Pairing again with
another PC adds a partner; `synergy-tools partners` lists them. Details: [docs/files.md](docs/files.md#pairing).

## Layout

The repo root is the `synergy-tools` crate: file sending (formerly synergy-dragndrop) and
audio (formerly synergy-audio), history kept via `git mv`.

```
src/                    the synergy-tools crate (audio.rs is the audio engine)
docs/                   files.md, audio.md, history/, superpowers/ (specs, plans)
.github/workflows/      ci.yml
```

## Build

```sh
git clone https://github.com/ztancrell/synergy-tools.git
cd synergy-tools && cargo test && cargo build --release
```

Linux build dependencies: `libgtk-3-dev libayatana-appindicator3-dev libasound2-dev`
(Arch `gtk3 libayatana-appindicator alsa-lib`). Windows builds are described in each tool's docs.

## CI and releases

`ci.yml` tests and builds the crate on every push and pull request.

Superseded: per-tool releases end here; phase 3 adds one release workflow.
