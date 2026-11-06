# synergy-tools

Small tools that fill gaps in [Synergy 3](https://symless.com/synergy), which shares keyboard
and mouse between computers but not audio or file drag-and-drop.

| Tool | What it does | Platforms |
|------|--------------|-----------|
| [synergy-tools audio](docs/audio.md) | Streams one machine's audio to another over the LAN (e.g. Windows game audio to Linux headphones). Part of the `synergy-tools` program. | Windows sender → Linux receiver |
| [synergy-tools files](docs/files.md) | Sends files between the two machines: drop window, right-click "Send to", tray. Part of the `synergy-tools` program at the repo root. | Windows ↔ Linux |

Both use one pairing (an 8-digit code, once), and their traffic is encrypted and
authenticated. Usage steps are in each tool's doc.

### Pairing

`synergy-tools` pairs with an 8-digit code (`synergy-tools pair` on one PC, `synergy-tools pair
<host> <code>` on the other). Pairing again with another PC adds a partner, so one PC can have
several; `synergy-tools partners` lists them. One code pairs audio and files. Details:
[docs/files.md](docs/files.md#pairing).

### Tray

The tray menu's first line shows `Not paired`, `No partner online` or `Online: <names>`; the app
pings its partners every 5 seconds. Dropped files go to the online partner, with a menu to pick
one when several are online. Details: [docs/files.md](docs/files.md#daily-use).

## Layout

The `synergy-tools` program is the Rust crate at the repo root (`Cargo.toml`, `src/`), formerly
`synergy-dragndrop`; the former `synergy-audio` crate is `src/audio.rs` (its README is
`docs/audio.md`). Both keep their full history from the former standalone repos, imported with
`git subtree add`. Commits from before the import touched paths at the old repo's root, so
`git log --follow -- src/audio.rs` stops at the import; to see the older ones, follow the import
merge (`git log --oneline 'HEAD^{/Add .synergy-audio/. from commit}^2'`).

```
src/                    the synergy-tools program
docs/                   docs (files.md, audio.md), design spec and plans, history/
.github/workflows/      ci.yml
```

## Build

```sh
git clone https://github.com/ztancrell/synergy-tools.git
cd synergy-tools && cargo test && cargo build --release
```

Linux build dependencies (Debian/Ubuntu): `libgtk-3-dev libayatana-appindicator3-dev
libasound2-dev` (Arch: `gtk3 libayatana-appindicator alsa-lib`). The Windows build is
described in [docs/files.md](docs/files.md#build-from-source).

## CI and releases

`ci.yml` builds and tests `synergy-tools` on Ubuntu 22.04 and Windows for every push to `main`
and every pull request.

Superseded: per-tool releases end here; phase 3 adds one release workflow. Old `audio-v*`
releases stay on the Releases page.
