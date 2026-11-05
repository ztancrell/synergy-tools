# synergy-tools

Small tools that fill gaps in [Synergy 3](https://symless.com/synergy), which shares keyboard
and mouse between computers but not audio or file drag-and-drop.

| Tool | What it does | Platforms |
|------|--------------|-----------|
| [synergy-audio](synergy-audio/) | Streams one machine's audio to another over the LAN (e.g. Windows game audio to Linux headphones). | Windows/macOS sender → Linux receiver |
| [synergy-dragndrop](synergy-dragndrop/) | Sends files between the two machines: drop window, right-click "Send to", tray. | Windows ↔ Linux |

Both pair the two machines once with an 8-digit code, and their traffic is encrypted and
authenticated. Install and usage steps are in each tool's README.

## Layout

One repo, one folder per tool. Each folder is a standalone Rust crate (own `Cargo.toml`,
`Cargo.lock`, `target/`, install scripts, README and LICENSE); there is no Cargo workspace.
Each folder keeps its full history from the tool's former standalone repo, imported with
`git subtree add`. Commits from before the import touched paths at the old repo's root, so
`git log -- synergy-audio` lists only later commits; to see the older ones, follow the import
merge (`git log --oneline 'HEAD^{/Add .synergy-audio/. from commit}^2'`).

```
synergy-audio/          the audio tool
synergy-dragndrop/      the file-sending tool
.github/workflows/      audio-ci.yml, audio-release.yml, dragndrop-ci.yml, dragndrop-release.yml
```

## Build

```sh
git clone https://github.com/ztancrell/synergy-tools.git
cd synergy-tools/synergy-audio && cargo test && cargo build --release
cd ../synergy-dragndrop && cargo test && cargo build --release
```

Linux build dependencies: synergy-audio needs ALSA headers (Debian/Ubuntu `libasound2-dev`,
Arch `alsa-lib`); synergy-dragndrop needs GTK 3 and Ayatana appindicator (Debian/Ubuntu
`libgtk-3-dev libayatana-appindicator3-dev`, Arch `gtk3 libayatana-appindicator`). Windows and
macOS builds are described in each tool's README.

## CI and releases

Each tool's CI runs only when a push to `main` or a pull request touches its folder or its own
workflow file; a push touching both folders runs both.

Releases are per tool, by tag prefix:

| Tag | Workflow | Release assets |
|-----|----------|----------------|
| `audio-vX.Y.Z` | `audio-release.yml` | `synergy-audio-vX.Y.Z-{linux-x86_64.tar.gz, windows-x86_64.zip, macos-universal.tar.gz}`, `SHA256SUMS` |
| `dragndrop-vX.Y.Z` | `dragndrop-release.yml` | `synergy-dragndrop-vX.Y.Z-{linux-x86_64.tar.gz, windows-x86_64.zip}`, `SHA256SUMS` |

`X.Y.Z` must equal the `version` in that tool's `Cargo.toml`, or the release fails. Example:

```sh
git tag audio-v0.2.0 && git push origin audio-v0.2.0
```
