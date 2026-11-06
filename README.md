# synergy-tools

Small tools that fill gaps in [Synergy 3](https://symless.com/synergy), which shares keyboard
and mouse between computers but not audio or file drag-and-drop.

| Tool | What it does | Platforms |
|------|--------------|-----------|
| [audio](docs/audio.md) (`synergy-tools audio`) | Streams one machine's audio to another over the LAN (e.g. Windows game audio to Linux headphones). | Windows or macOS sender → Linux receiver |
| [files](docs/files.md) (`synergy-tools`) | Sends files between the two machines: drop window, tray, `synergy-tools files send` (right-click "Send to other PC" in Thunar/Dolphin/Nautilus, Explorer and Finder via `synergy-tools install`). | Windows, macOS, Linux (any pair) |

Run `synergy-tools` with no arguments for the tray (one icon for both tools; on macOS it sits in
the menu bar, with no Dock icon). `synergy-tools install` copies it to a per-user place
(`~/.local/bin`, `%LOCALAPPDATA%\Programs\synergy-tools`, the app bundle `~/Applications/synergy-tools.app` on macOS), starts it at login and adds the right-click entries ([docs/files.md](docs/files.md#install)). Its menu:

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
Update
Quit
```

Update downloads the latest build, swaps it in and restarts the tray (see [Updates](#updates)).
Dropped files go to the online partner (a picker when several are online). Audio streams by
itself from Windows or macOS to the online Linux partner ([docs/audio.md](docs/audio.md#in-the-tray)).

Both pair the two machines once with an 8-digit code, and their traffic is encrypted and
authenticated. Usage: [docs/files.md](docs/files.md) and [docs/audio.md](docs/audio.md).

## Pairing

One code pairs two PCs: run `synergy-tools pair` on one and `synergy-tools pair <host> <code>`
on the other. The pairing covers audio and files alike. Pairing again with
another PC adds a partner; `synergy-tools partners` lists them. Details: [docs/files.md](docs/files.md#pairing).

## Updates

The tray's **Update** item, or `synergy-tools update` from a terminal, downloads this OS's build
from the rolling `latest` GitHub release with the system `curl` (`curl.exe` on Windows 10 1803 and
later), checks it (an executable for this OS that runs and prints its `--version`) and swaps it in
for the running program (the old one is moved to `<program>.old` first and moved back on failure).
It says `Already up to date: …` when the versions match, else `Updated: … -> …`; the tray then
starts the new version and quits. On macOS the app bundle is re-signed ad hoc, as `install` does.
Nothing checks for updates by itself. Details: [docs/files.md](docs/files.md#updates).

Both machines speak one protocol: **update every machine** (each tray's Update item, or
`synergy-tools update` on each). Builds from before this feature have no Update: install those
once by hand (download, `install`).

## Layout

The repo root is the `synergy-tools` crate: file sending (formerly synergy-dragndrop) and
audio (formerly synergy-audio), history kept via `git mv`.

```
src/                    the synergy-tools crate (audio.rs is the audio engine)
docs/                   files.md, audio.md, history/, superpowers/ (specs, plans)
.github/workflows/      ci.yml, release.yml
build.rs                puts the commit's short hash into --version
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

`ci.yml` tests and builds the crate on every push to a branch other than `main` and on every
pull request, in three jobs: `linux`,
`windows` and `macos`. Warnings fail CI (`RUSTFLAGS=-D warnings`). The `macos` job also uploads a
universal binary (arm64 + x86_64); all three jobs upload their binary as a workflow artifact
(`synergy-tools-linux-x86_64`, `synergy-tools-windows-x86_64`, `synergy-tools-macos-universal`).

A release: bump `version` in `Cargo.toml`, commit, then `git tag v<version> && git push origin v<version>`.
`.github/workflows/release.yml` builds with `ci.yml` and publishes the three assets
(`synergy-tools-linux-x86_64`, `synergy-tools-windows-x86_64.exe`, `synergy-tools-macos-universal`)
to a GitHub Release; the tag must match the Cargo version. The binaries are unsigned: Mac users remove
the quarantine flag (`xattr -d com.apple.quarantine synergy-tools-macos-universal`, see
[files.md](docs/files.md#macos)); Windows users get SmartScreen (More info > Run anyway).

Every push to `main` that changes the program (`src/**`, `build.rs`, `Cargo.toml`, `Cargo.lock`,
the workflows) runs `release.yml`: it builds with `ci.yml` (so `main` is built once, not twice) and
publishes the same three assets to the rolling `latest` release (tag `latest` force-moved to the
commit, title `latest (<short hash>)`), which `synergy-tools update` downloads. `--version` prints
the Cargo version plus that short hash (`build.rs`), so update can tell two builds apart.

Superseded: `ci.yml` running on pushes to `main` too; `release.yml` builds `main` now.

Superseded: per-tool tags `audio-v*` / `dragndrop-v*` and their workflows; one `v*` tag now
(`.github/workflows/release.yml`).
