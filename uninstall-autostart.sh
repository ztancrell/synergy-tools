#!/usr/bin/env bash
# Undo install-autostart.sh: stop and disable the "synergy-audio" user service, remove its unit.
# Leaves the binary (~/.local/bin/synergy-audio) and the firewall rule; the output says how
# to remove them.
# Usage: ./uninstall-autostart.sh
set -euo pipefail

name=synergy-audio
unit="$HOME/.config/systemd/user/$name.service"

if [ -f "$unit" ]; then
    systemctl --user disable --now "$name" >/dev/null 2>&1 || true
    rm -f "$unit"
    systemctl --user daemon-reload
    echo "Removed user service '$name': the receiver is stopped and no longer starts at login."
else
    echo "No user service '$name' found (already uninstalled)."
fi

echo "Left in place. To remove them too:"
echo "  rm ~/.local/bin/$name"
echo "  sudo ufw delete allow 24850/udp   # firewalld: sudo firewall-cmd --permanent --remove-port=24850/udp && sudo firewall-cmd --reload"
