#!/usr/bin/env bash
set -euo pipefail

# evo-manager-gtk Install — baut und installiert das EVO-X2 Tray-Widget

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN="$WORKSPACE_ROOT/target/release/evo-manager-widget"

echo "=== EVO-X2 Manager Widget Install ==="

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

echo ""
echo "EVO-X2 Manager installiert."
echo "  Binary:    ~/.local/bin/evo-manager-widget"
echo "  Autostart: ~/.config/autostart/evo-manager.desktop"
echo ""
echo "Starten: evo-manager-widget"
echo ""
echo "Config:  ~/.config/evo-manager/config.json"
echo '  Beispiel: {"evo_ip": "192.168.178.100", "metrics_port": 8084, "ssh_user": "jan"}'
