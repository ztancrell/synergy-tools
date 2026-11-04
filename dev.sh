#!/usr/bin/env bash
# Dev convenience: build Linux + Windows binaries, then run the receiver.
# Usage: ./dev.sh [--buffer-ms N]
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release
cargo build --release --target x86_64-pc-windows-gnu
exec target/release/synergy-audio receive "$@"
