#!/usr/bin/env bash
# Run the receiver permanently on Linux: a systemd *user* service "synergy-audio" that
# starts at login (it needs your PipeWire session), restarts on failure, logs to the journal.
# Builds the release binary, copies it to ~/.local/bin, writes the unit, (re)starts it, and
# opens UDP 24850 in ufw/firewalld if needed (only that asks for your sudo password).
# Rerun to reinstall or update; dev.sh does that for you when the service is installed.
# Usage: ./install-autostart.sh [--buffer-ms N]   (default: keep the installed value, else 40)
set -euo pipefail
cd "$(dirname "$0")"

name=synergy-audio
port=24850
bin="$HOME/.local/bin/$name"
unit="$HOME/.config/systemd/user/$name.service"

buffer=""
while [ $# -gt 0 ]; do
    case "$1" in
        --buffer-ms) buffer="${2:?--buffer-ms needs a number}"; shift 2 ;;
        --buffer-ms=*) buffer="${1#*=}"; shift ;;
        *) echo "Unknown argument: $1 (usage: $0 [--buffer-ms N])" >&2; exit 1 ;;
    esac
done
if [ -z "$buffer" ] && [ -f "$unit" ]; then
    buffer=$(sed -n 's/^ExecStart=.*--buffer-ms \([0-9]*\).*/\1/p' "$unit")
fi
buffer="${buffer:-40}"
case "$buffer" in ''|*[!0-9]*) echo "--buffer-ms must be a whole number, got '$buffer'" >&2; exit 1 ;; esac

unit_text() {
    cat <<EOF
[Unit]
Description=synergy-audio receiver (plays audio streamed from the Windows sender, UDP $port)
Wants=pipewire.service
After=pipewire.service pipewire-pulse.service

[Service]
ExecStart=%h/.local/bin/$name receive --buffer-ms $1
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
EOF
}

cargo build --release

# Copy to a temp name, then rename: a plain cp over the running binary fails ("Text file busy").
mkdir -p "$(dirname "$bin")" "$(dirname "$unit")"
install -m755 "target/release/$name" "$bin.new"
mv -f "$bin.new" "$bin"
unit_text "$buffer" > "$unit"

# A receiver started by hand holds the port, so the service couldn't bind it: stop those.
service_pid=$(systemctl --user show -p MainPID --value "$name" 2>/dev/null || echo 0)
for pid in $(pgrep -f "(^|/)$name receive" || true); do
    if [ "$pid" != "$service_pid" ]; then
        echo "Stopping receiver started by hand (pid $pid) so the service can use port $port."
        kill "$pid" || true
    fi
done

# Firewall: open UDP $port only if it isn't open yet (the checks need no sudo).
if grep -qs '^ENABLED=yes' /etc/ufw/ufw.conf; then
    if ! grep -qsE "^### tuple ### allow (udp|any) $port " /etc/ufw/user.rules; then
        echo "ufw is active and UDP $port isn't open: sudo asks for your password to run 'ufw allow $port/udp'."
        sudo ufw allow "$port/udp"
    fi
elif command -v firewall-cmd >/dev/null && firewall-cmd --state >/dev/null 2>&1; then
    if ! firewall-cmd --permanent --query-port="$port/udp" >/dev/null 2>&1; then
        echo "firewalld is active and UDP $port isn't open: sudo asks for your password to open it."
        sudo firewall-cmd --permanent --add-port="$port/udp"
        sudo firewall-cmd --reload
    fi
else
    echo "No active ufw/firewalld found; not touching the firewall."
fi

systemctl --user daemon-reload
systemctl --user enable "$name"
systemctl --user restart "$name"   # restart, so a reinstall picks up the new binary/buffer
sleep 2

echo ""
echo "Installed: user service '$name' runs '$bin receive --buffer-ms $buffer'"
echo "at every login (restarts 3 s after a crash). Unit: $unit"
systemctl --user status "$name" --no-pager -n 3 || true
echo ""
echo "Status:   systemctl --user status $name"
echo "Live log: journalctl --user -u $name -f -o cat   (Ctrl+C stops only the log view)"
echo "Remove:   ./uninstall-autostart.sh"
