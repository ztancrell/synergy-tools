#!/usr/bin/env bash
# Undo install-autostart.sh: stop the app, remove the binary, the autostart entry (XDG file and/or
# the marked line in ~/.config/openbox/autostart) and the Thunar/Dolphin/Nautilus entries.
# Leaves your pairing config and the firewall rule (prints the command to close it).
set -euo pipefail

name=synergy-dragndrop
cfg="${XDG_CONFIG_HOME:-$HOME/.config}"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
ob_autostart="$cfg/openbox/autostart"

pkill -fx "$HOME/.local/bin/$name" 2>/dev/null || true
rm -f "$HOME/.local/bin/$name" "$HOME/.local/bin/$name.new" \
      "$cfg/autostart/$name.desktop" \
      "$data/Thunar/sendto/$name.desktop" \
      "$data/kio/servicemenus/$name.desktop" \
      "$data/nautilus/scripts/Send to other PC"
if [ -f "$ob_autostart" ]; then
    sed -i "\| & # $name\$|d" "$ob_autostart"
fi
echo "Removed $name (binary, autostart, right-click entries). Config kept in $cfg/$name."
echo "The firewall rule stays; to close it: sudo ufw delete allow 24851/tcp   (firewalld: sudo firewall-cmd --permanent --remove-port=24851/tcp && sudo firewall-cmd --reload)"
