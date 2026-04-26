#!/usr/bin/env bash
set -euo pipefail

# flowaudit-launcher-gtk Install — baut und installiert das FlowAudit Tray-Widget

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN="$WORKSPACE_ROOT/target/release/flowaudit-launcher-widget"

echo "=== FlowAudit Launcher Widget Install ==="

# Abhaengigkeiten pruefen
echo "Pruefe Abhaengigkeiten..."
MISSING=()
dpkg -l libgtk-3-dev &>/dev/null || MISSING+=("libgtk-3-dev")
dpkg -l libayatana-appindicator3-dev &>/dev/null \
    || dpkg -l libappindicator3-dev &>/dev/null \
    || MISSING+=("libayatana-appindicator3-dev")
command -v cargo &>/dev/null || MISSING+=("cargo (rustup)")

if [[ ${#MISSING[@]} -gt 0 ]]; then
    echo "Fehlende Pakete: ${MISSING[*]}"
    echo "Installieren mit: sudo apt install ${MISSING[*]}"
    exit 1
fi
echo "  Alle Abhaengigkeiten vorhanden."

# Build im Workspace-Kontext
if [[ ! -f "$BIN" ]] || [[ "$SCRIPT_DIR/src/main.rs" -nt "$BIN" ]]; then
    echo "Baue Release..."
    cd "$WORKSPACE_ROOT"
    cargo build --release -p flowaudit-launcher-gtk
fi

echo "Installiere Binary..."
mkdir -p "$HOME/.local/bin"
install -m 755 "$BIN" "$HOME/.local/bin/flowaudit-launcher-widget"

echo "Installiere Autostart..."
mkdir -p "$HOME/.config/autostart"
cat > "$HOME/.config/autostart/flowaudit-launcher.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=FlowAudit Apps
Comment=FlowAudit App Launcher (Tray-Widget)
Exec=$HOME/.local/bin/flowaudit-launcher-widget
Terminal=false
Categories=System;Network;
StartupNotify=false
X-GNOME-Autostart-enabled=true
EOF

echo ""
echo "FlowAudit Launcher Widget installiert."
echo "  Binary:    ~/.local/bin/flowaudit-launcher-widget"
echo "  Autostart: ~/.config/autostart/flowaudit-launcher.desktop"
echo "  apps.yml:  \$HOME/Projekte/flowlib/scripts/desktop-apps/apps.yml"
echo "             (per FLOWAUDIT_APPS_YML ueberschreibbar)"
echo ""
echo "Starten:     flowaudit-launcher-widget"
echo "Oder:        Neustart fuer Autostart"
