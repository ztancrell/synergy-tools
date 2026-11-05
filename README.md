# synergy-dragndrop

[![CI](https://github.com/ztancrell/synergy-dragndrop/actions/workflows/ci.yml/badge.svg)](https://github.com/ztancrell/synergy-dragndrop/actions/workflows/ci.yml)

Sends files between a Windows PC and a Linux PC on the same LAN, for machines that share
keyboard and mouse with Synergy 3 (which cannot drag files across). Both machines run the
same program: each one can send files to, and receive files from, the one paired partner.
Files land in the Downloads folder; transfers are authenticated and encrypted after a one-time
pairing with an 8-digit code.

**Status: in development.** Only the scaffold and the peer config exist so far; nothing can be
sent yet.

## Where files go

Received files are saved in your Downloads folder.

- A file is written under a hidden temporary name (`.name.xxxxxxxx.part`) and only gets its real
  name once it has arrived completely, so you never see half a file.
- An existing file is never replaced: a second `report.pdf` is saved as `report (1).pdf`,
  then `report (2).pdf`, and so on.
- File names from the other PC are cleaned up: folders in the name are dropped, and characters
  Windows does not allow (`< > : " | ? *`) become `_`.
- On Windows, received files are marked as downloaded from the internet (Mark of the Web,
  `ZoneId=3`), so SmartScreen may ask before you open an executable. On Linux, received files
  are never executable.

## Build from source

```sh
cargo build --release
cargo test
```

The binary is `target/release/synergy-dragndrop`. Run it with no arguments for usage.
Windows build check from Linux: `cargo check --target x86_64-pc-windows-gnu`.

## License

GPL-3.0-or-later, see [LICENSE](LICENSE).
