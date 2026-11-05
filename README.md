# synergy-dragndrop

[![CI](https://github.com/ztancrell/synergy-dragndrop/actions/workflows/ci.yml/badge.svg)](https://github.com/ztancrell/synergy-dragndrop/actions/workflows/ci.yml)

Sends files between a Windows PC and a Linux PC on the same LAN, for machines that share
keyboard and mouse with Synergy 3 (which cannot drag files across). Both machines run the
same program: each one can send files to, and receive files from, the one paired partner.
Files land in the Downloads folder; transfers are authenticated and encrypted after a one-time
pairing with an 8-digit code.

**Status: in development.** Only the scaffold and the peer config exist so far; nothing can be
sent yet.

## Build from source

```sh
cargo build --release
cargo test
```

The binary is `target/release/synergy-dragndrop`. Run it with no arguments for usage.
Windows build check from Linux: `cargo check --target x86_64-pc-windows-gnu`.

## License

GPL-3.0-or-later, see [LICENSE](LICENSE).
