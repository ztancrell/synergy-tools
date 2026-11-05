# synergy-tools

Small tools that fill gaps in [Synergy 3](https://symless.com/synergy), which shares keyboard
and mouse between computers but not audio or file drag-and-drop.

| Tool | What it does | Platforms |
|------|--------------|-----------|
| [synergy-audio](https://github.com/ztancrell/synergy-audio) | Streams one machine's audio to another over the LAN (e.g. Windows game audio to Linux headphones). | Windows/macOS sender → Linux receiver |
| [synergy-dragndrop](https://github.com/ztancrell/synergy-dragndrop) | Sends files between the two machines: drop window, right-click "Send to", tray. | Windows ↔ Linux |

Both pair the two machines once with an 8-digit code, and their traffic is encrypted and
authenticated. Each tool has its own repo, releases and install steps; see its README.

Each tool is tracked here as a git submodule. To get everything in one clone:

```sh
git clone --recurse-submodules https://github.com/ztancrell/synergy-tools.git
```

To move the submodules to the latest commit of their tracked branch:

```sh
git submodule update --remote
```
