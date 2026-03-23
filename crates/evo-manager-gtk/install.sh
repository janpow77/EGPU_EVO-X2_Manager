#!/usr/bin/env bash
set -euo pipefail

# evo-manager-gtk Install — baut und installiert das EVO-X2 Tray-Widget auf dem NUC

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN="$WORKSPACE_ROOT/target/release/evo-manager-widget"

echo "=== EVO-X2 Manager Widget Install ==="

# Abhaengigkeiten pruefen
echo "Pruefe Abhaengigkeiten..."
MISSING=()
dpkg -l libgtk-3-dev &>/dev/null || MISSING+=("libgtk-3-dev")
dpkg -l libappindicator3-dev &>/dev/null || MISSING+=("libappindicator3-dev")
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
    cargo build --release -p evo-manager-gtk
fi

echo "Installiere Binary..."
mkdir -p "$HOME/.local/bin"
install -m 755 "$BIN" "$HOME/.local/bin/evo-manager-widget"

echo "Installiere Autostart..."
mkdir -p "$HOME/.config/autostart"
cat > "$HOME/.config/autostart/evo-manager.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=EVO-X2 Manager
Comment=GMKtec EVO-X2 LLM Server Monitoring
Exec=$HOME/.local/bin/evo-manager-widget
Terminal=false
Categories=System;Monitor;
StartupNotify=false
X-GNOME-Autostart-enabled=true
EOF

# Config-Template erstellen falls nicht vorhanden
CONFIG_DIR="$HOME/.config/evo-manager"
CONFIG_FILE="$CONFIG_DIR/config.json"
if [[ ! -f "$CONFIG_FILE" ]]; then
    echo "Erstelle Config-Template..."
    mkdir -p "$CONFIG_DIR"
    cat > "$CONFIG_FILE" <<'CONF'
{
  "evo_ip": "",
  "metrics_port": 8084,
  "ssh_user": "jan",
  "poll_interval_secs": 5,
  "github_url": "",
  "setup_dir": ""
}
CONF
    # setup_dir automatisch setzen
    SETUP_DIR="$(cd "$WORKSPACE_ROOT/../../evo/setup" 2>/dev/null && pwd || echo "")"
    if [[ -n "$SETUP_DIR" ]]; then
        sed -i "s|\"setup_dir\": \"\"|\"setup_dir\": \"$SETUP_DIR\"|" "$CONFIG_FILE"
    fi
    echo "  Config erstellt: $CONFIG_FILE"
    echo "  Bitte evo_ip eintragen (LAN-IP der EVO-X2)!"
fi

echo ""
echo "EVO-X2 Manager installiert."
echo "  Binary:    ~/.local/bin/evo-manager-widget"
echo "  Autostart: ~/.config/autostart/evo-manager.desktop"
echo "  Config:    $CONFIG_FILE"
echo ""
echo "Starten:     evo-manager-widget"
echo "Oder:        Neustart fuer Autostart"
