# synergy-tools

Small tools that fill gaps in [Synergy 3](https://symless.com/synergy), which shares keyboard
and mouse between computers but not audio or file drag-and-drop.

| Tool | What it does | Platforms |
|------|--------------|-----------|
| [audio](docs/audio.md) (`synergy-tools audio`) | Streams one machine's audio to another over the LAN (e.g. Windows game audio to Linux headphones). | Windows or macOS sender → Linux receiver |
| [files](docs/files.md) (`synergy-tools`) | Sends files between the two machines: drop window, tray, `synergy-tools files send` (right-click Send To entries come back with phase 3's installer). | Windows, macOS, Linux (any pair) |

Run `synergy-tools` with no arguments for the tray (one icon for both tools; on macOS it sits in
the menu bar, with no Dock icon). Its menu:

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
itself from Windows or macOS to the online Linux partner ([docs/audio.md](docs/audio.md#in-the-tray)).

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
(Arch `gtk3 libayatana-appindicator alsa-lib`). macOS needs `xcode-select --install` and rustup.
Windows builds are described in each tool's docs.

## CI and releases

`ci.yml` tests and builds the crate on every push and pull request in three jobs: `linux`,
`windows` and `macos`. Warnings fail CI (`RUSTFLAGS=-D warnings`). The `macos` job also uploads a
universal binary (arm64 + x86_64) as the workflow artifact `synergy-tools-macos-universal`: unsigned,
downloadable from the run's page, not a release.

Superseded: per-tool releases end here; phase 3 adds one release workflow.
