# synergy-tools

Small tools that fill gaps in [Synergy 3](https://symless.com/synergy), which shares keyboard
and mouse between computers but not audio or file drag-and-drop.

| Tool | What it does | Platforms |
|------|--------------|-----------|
| [synergy-audio](synergy-audio/) | Streams one machine's audio to another over the LAN (e.g. Windows game audio to Linux headphones). | Windows/macOS sender → Linux receiver |
| [synergy-tools files](docs/files.md) | Sends files between the two machines: drop window, right-click "Send to", tray. Part of the `synergy-tools` program at the repo root. | Windows ↔ Linux |

Both pair the two machines once with an 8-digit code, and their traffic is encrypted and
authenticated. Usage steps are in each tool's README.

## Layout

The `synergy-tools` program is the Rust crate at the repo root (`Cargo.toml`, `src/`), formerly
`synergy-dragndrop`. `synergy-audio/` is still a standalone crate (own `Cargo.toml`,
`Cargo.lock`, `target/`, install scripts, README and LICENSE) until it is absorbed into
`synergy-tools`; there is no Cargo workspace. Both keep their full history from the former
standalone repos, imported with `git subtree add`. Commits from before the import touched paths
at the old repo's root, so `git log -- synergy-audio` lists only later commits; to see the older
ones, follow the import merge (`git log --oneline 'HEAD^{/Add .synergy-audio/. from commit}^2'`).

```
src/                    the synergy-tools program
docs/                   docs (files.md), design spec and plans, history/
synergy-audio/          the audio tool (until it moves into synergy-tools)
.github/workflows/      ci.yml, audio-ci.yml, audio-release.yml
```

## Build

```sh
git clone https://github.com/ztancrell/synergy-tools.git
cd synergy-tools && cargo test && cargo build --release
cd synergy-audio && cargo test && cargo build --release
```

Linux build dependencies (Debian/Ubuntu): `libgtk-3-dev libayatana-appindicator3-dev
libasound2-dev` (Arch: `gtk3 libayatana-appindicator alsa-lib`). Windows and macOS builds are
described in each tool's README.

## CI and releases

`ci.yml` builds and tests `synergy-tools` on Ubuntu 22.04 and Windows for every push to `main`
and every pull request. `audio-ci.yml` runs only when a push to `main` or a pull request touches
`synergy-audio/` or its own workflow file.

`synergy-tools` has no release workflow yet. Audio releases are by tag:

| Tag | Workflow | Release assets |
|-----|----------|----------------|
| `audio-vX.Y.Z` | `audio-release.yml` | `synergy-audio-vX.Y.Z-{linux-x86_64.tar.gz, windows-x86_64.zip, macos-universal.tar.gz}`, `SHA256SUMS` |

`X.Y.Z` must equal the `version` in `synergy-audio/Cargo.toml`, or the release fails. Example:

```sh
git tag audio-v0.2.0 && git push origin audio-v0.2.0
```
