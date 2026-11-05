#!/usr/bin/env bash
# Install synergy-dragndrop on Linux: binary in ~/.local/bin, tray app started at login,
# a right-click "Send to other PC" entry, and TCP 24851 opened in ufw/firewalld (only that asks
# for your sudo password). Builds from source if Cargo.toml is next to this script, else uses
# the bundled binary. Rerun to update.
#
# Right-click entries: Thunar (~/.local/share/Thunar/sendto, the Openbox desktop's file manager),
# plus Dolphin (kio servicemenu) and Nautilus (scripts menu).
#
# Autostart, exactly one mechanism so the app is never launched twice:
#   - default: XDG entry ~/.config/autostart/synergy-dragndrop.desktop. Openbox runs these
#     through openbox-xdg-autostart (needs PyXDG), as do KDE/GNOME/XFCE.
#   - fallback: if ~/.config/openbox/autostart exists but no openbox-xdg-autostart is found,
#     one line "<bin> & # synergy-dragndrop" is appended to that file instead (no XDG entry).
#
# Test switches (not for normal use): SDD_SKIP_FIREWALL=1, SDD_NO_START=1,
# OPENBOX_XDG_AUTOSTART=<path> (the only runner path to look for).
set -euo pipefail
cd "$(dirname "$0")"

name=synergy-dragndrop
port=24851
bin="$HOME/.local/bin/$name"
cfg="${XDG_CONFIG_HOME:-$HOME/.config}"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
xdg_entry="$cfg/autostart/$name.desktop"
ob_autostart="$cfg/openbox/autostart"
marker="# $name"

if [ -f Cargo.toml ]; then
    command -v cargo >/dev/null || { echo "cargo not found; install Rust, or use the prebuilt release tarball (it has the binary next to this script)." >&2; exit 1; }
    cargo build --release
    src="target/release/$name"
elif [ -f "$name" ]; then
    src="$name"
else
    echo "No '$name' binary next to this script and no Cargo.toml to build one from." >&2
    exit 1
fi

# Temp name + rename: a plain cp over the running binary fails ("Text file busy").
mkdir -p "$(dirname "$bin")"
install -m755 "$src" "$bin.new"
mv -f "$bin.new" "$bin"

# Right-click entries.
mkdir -p "$data/Thunar/sendto" "$data/kio/servicemenus" "$data/nautilus/scripts"
cat > "$data/Thunar/sendto/$name.desktop" <<EOT
[Desktop Entry]
Type=Application
Name=Send to other PC
Icon=document-send
Exec="$bin" send %F
MimeType=application/octet-stream;
EOT
cat > "$data/kio/servicemenus/$name.desktop" <<EOT
[Desktop Entry]
Type=Service
MimeType=application/octet-stream;
X-KDE-ServiceTypes=KonqPopupMenu/Plugin
Actions=send

[Desktop Action send]
Name=Send to other PC
Icon=document-send
Exec="$bin" send %F
EOT
chmod +x "$data/kio/servicemenus/$name.desktop"   # Plasma 6 requires it
printf '#!/bin/sh\nexec "%s" send "$@"\n' "$bin" > "$data/nautilus/scripts/Send to other PC"
chmod +x "$data/nautilus/scripts/Send to other PC"

# Autostart (see header).
xdg_ok=
if [ -n "${OPENBOX_XDG_AUTOSTART+x}" ]; then
    [ -x "$OPENBOX_XDG_AUTOSTART" ] && xdg_ok=1
else
    for p in /usr/local/libexec/openbox-xdg-autostart /usr/libexec/openbox-xdg-autostart \
             /usr/lib/openbox/openbox-xdg-autostart /usr/bin/openbox-xdg-autostart; do
        [ -x "$p" ] && xdg_ok=1
    done
fi
if [ -f "$ob_autostart" ] && [ -z "$xdg_ok" ]; then
    rm -f "$xdg_entry"
    if ! grep -qxF "$bin & $marker" "$ob_autostart"; then
        # Own line even if the file lacks a trailing newline.
        [ -z "$(tail -c1 "$ob_autostart")" ] || echo >> "$ob_autostart"
        echo "$bin & $marker" >> "$ob_autostart"
    fi
    echo "Autostart: line added to $ob_autostart (no openbox-xdg-autostart found)."
else
    [ ! -f "$ob_autostart" ] || sed -i "\| & $marker\$|d" "$ob_autostart"   # flipped from the fallback: never start twice
    mkdir -p "$(dirname "$xdg_entry")"
    cat > "$xdg_entry" <<EOT
[Desktop Entry]
Type=Application
Name=synergy-dragndrop
Comment=Send files to the other PC
Exec="$bin"
Terminal=false
X-GNOME-Autostart-enabled=true
EOT
    echo "Autostart: $xdg_entry"
fi

# Firewall: open TCP $port only if it isn't open yet (the checks need no sudo).
if [ -n "${SDD_SKIP_FIREWALL:-}" ]; then
    echo "Skipping the firewall (SDD_SKIP_FIREWALL)."
elif grep -qs '^ENABLED=yes' /etc/ufw/ufw.conf; then
    if ! grep -qsE "^### tuple ### allow (tcp|any) $port " /etc/ufw/user.rules; then
        echo "ufw is active and TCP $port isn't open: sudo asks for your password to run 'ufw allow $port/tcp'."
        sudo ufw allow "$port/tcp"
    fi
elif command -v firewall-cmd >/dev/null && firewall-cmd --state >/dev/null 2>&1; then
    if ! firewall-cmd --permanent --query-port="$port/tcp" >/dev/null 2>&1; then
        echo "firewalld is active and TCP $port isn't open: sudo asks for your password to open it."
        sudo firewall-cmd --permanent --add-port="$port/tcp"
        sudo firewall-cmd --reload
    fi
else
    echo "No active ufw/firewalld found; not touching the firewall."
fi

if [ -n "${SDD_NO_START:-}" ]; then
    :
elif pgrep -fx "$bin" >/dev/null; then
    echo "Already running; restart it (or log out and in) to use the new binary."
else
    setsid -f "$bin" >/dev/null 2>&1
    echo "Started the tray app."
fi

echo "Installed $bin. Right-click a file > Send to other PC (Thunar: Send To). Remove: ./uninstall-autostart.sh"
