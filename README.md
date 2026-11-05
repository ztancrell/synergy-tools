# synergy-dragndrop

[![CI](https://github.com/ztancrell/synergy-dragndrop/actions/workflows/ci.yml/badge.svg)](https://github.com/ztancrell/synergy-dragndrop/actions/workflows/ci.yml)

Sends files between a Windows PC and a Linux PC on the same LAN, for machines that share
keyboard and mouse with Synergy 3 (which cannot drag files across). Both machines run the
same program: each one can send files to, and receive files from, the one paired partner.
Files land in the Downloads folder; transfers are authenticated and encrypted after a one-time
pairing with an 8-digit code.

**Status: in development.** Pairing and sending work from the command line (below); the tray
app and drop window are not built yet.

## Pairing

Pair once; the pairing is remembered on both PCs. The two PCs must be able to reach each other on
TCP port **24851**, so allow that port in the firewall of the PC that receives (the installers
will do this; until then do it by hand).

1. On PC 1 run `synergy-dragndrop pair`. It prints a code like `1234-5678` and the command to
   run on the other PC. The code works for one attempt and expires after 2 minutes.
2. On PC 2 run `synergy-dragndrop pair <PC 1's address> 1234-5678`.

A wrong code fails on both sides and the code is spent: start again with a new one.
If a PC's address changes, run `synergy-dragndrop peer <new address>` on the other PC (the
secret is kept); `synergy-dragndrop unpair` forgets the pairing.

## Command line

```
synergy-dragndrop pair               show a code and wait for the other PC to pair
synergy-dragndrop pair <host> <code> pair with the PC showing <code>
synergy-dragndrop peer <host>        change the paired PC's address
synergy-dragndrop unpair             forget the paired PC
synergy-dragndrop send <file>...     send files to the paired PC
synergy-dragndrop listen             receive files (saved to Downloads) until stopped
```

`<host>` is an IP address or name, optionally `:port` (default 24851). `send` takes files only
(folders are refused: zip them first) and fails at once, within 3 seconds, if the other PC is
unreachable; nothing is queued or retried. If the tray app is already running it holds port
24851, so `pair` and `listen` say so; use its tray menu or quit it first.

## Notifications

Results show as desktop notifications, since "Send to" has no terminal: `send` reports success or the error, and a received transfer shows "N files received from <other PC>". Clicking that notification opens the Downloads folder (`xdg-open` on Linux, `explorer.exe` on Windows). If a notification can't be shown, the reason goes to stderr and the transfer is unaffected.

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
