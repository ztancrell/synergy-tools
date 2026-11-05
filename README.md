# synergy-tools

Small tools that fill gaps in [Synergy 3](https://symless.com/synergy), which shares keyboard
and mouse between computers but not audio or file drag-and-drop.

| Tool | What it does | Platforms |
|------|--------------|-----------|
| [synergy-audio](synergy-audio/) | Streams one machine's audio to another over the LAN (e.g. Windows game audio to Linux headphones). | Windows/macOS sender → Linux receiver |
| [files](docs/files.md) (`synergy-tools`) | Sends files between the two machines: drop window, right-click "Send to", tray. | Windows ↔ Linux |

Both pair the two machines once with an 8-digit code, and their traffic is encrypted and
authenticated. Usage: [docs/files.md](docs/files.md) and [synergy-audio/](synergy-audio/).

## Layout

The repo root is the `synergy-tools` crate (the file-sending tool, formerly synergy-dragndrop,
history kept via `git mv`). `synergy-audio/` is still a standalone crate until it is absorbed
into the root crate.

```
src/                    the synergy-tools crate
docs/                   files.md (file-sending guide), history/, superpowers/ (specs, plans)
synergy-audio/          the audio tool (standalone crate, until Task 4)
.github/workflows/      ci.yml, audio-ci.yml, audio-release.yml
```

## Build

```sh
git clone https://github.com/ztancrell/synergy-tools.git
cd synergy-tools && cargo test && cargo build --release
cd synergy-audio && cargo test && cargo build --release
```

Linux build dependencies: GTK 3, Ayatana appindicator and ALSA headers (Debian/Ubuntu
`libgtk-3-dev libayatana-appindicator3-dev libasound2-dev`, Arch `gtk3 libayatana-appindicator
alsa-lib`). Windows builds are described in each tool's docs.

## CI and releases

`ci.yml` tests and builds the root crate on every push and pull request; `audio-ci.yml` runs
only when `synergy-audio/` changes. Only synergy-audio has a release workflow for now:

| Tag | Workflow | Release assets |
|-----|----------|----------------|
| `audio-vX.Y.Z` | `audio-release.yml` | `synergy-audio-vX.Y.Z-{linux-x86_64.tar.gz, windows-x86_64.zip, macos-universal.tar.gz}`, `SHA256SUMS` |

`X.Y.Z` must equal the `version` in `synergy-audio/Cargo.toml`, or the release fails. Example:

```sh
git tag audio-v0.2.0 && git push origin audio-v0.2.0
```
