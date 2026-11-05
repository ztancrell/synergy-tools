#!/usr/bin/env bash
# Dev convenience: build Linux + Windows binaries, then run the receiver.
# If the user service is installed (install-autostart.sh), update and restart it instead
# and follow its log; otherwise run the receiver in the foreground.
# Usage: ./dev.sh [--buffer-ms N]
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release
cargo build --release --target x86_64-pc-windows-gnu
if [ -f "$HOME/.config/systemd/user/synergy-audio.service" ]; then
    ./install-autostart.sh "$@"   # copies the new binary, keeps the buffer unless --buffer-ms
    echo ""
    echo "Following the service log. Ctrl+C stops only the log view; the receiver keeps running."
    exec journalctl --user -u synergy-audio -f -o cat
fi
exec target/release/synergy-audio receive "$@"
