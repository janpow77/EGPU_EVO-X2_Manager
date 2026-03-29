#!/bin/bash
# setup-macos.sh
# Einmalig auf dem MacBook ausfuehren.
# Installiert Abhaengigkeiten, baut das Projekt, richtet SSH ein.

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo -e "${GREEN}=== VPN Router - macOS Setup ===${NC}"
echo ""

# 1. Homebrew pruefen
if ! command -v brew &>/dev/null; then
    echo -e "${RED}Homebrew nicht installiert.${NC}"
    echo 'Installiere mit: /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"'
    exit 1
fi
echo -e "${GREEN}Homebrew: OK${NC}"

# 2. Rust pruefen/installieren
if ! command -v cargo &>/dev/null; then
    echo "Installiere Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi
echo -e "${GREEN}Rust: $(rustc --version)${NC}"

# 3. sshpass installieren (fuer Notfall-Restore mit Passwort)
if ! command -v sshpass &>/dev/null; then
    echo "Installiere sshpass..."
    brew install hudochenkov/sshpass/sshpass 2>/dev/null || brew install esolitos/ipa/sshpass 2>/dev/null || {
        echo -e "${RED}sshpass konnte nicht installiert werden - Passwort-Login nicht moeglich.${NC}"
        echo "SSH-Key-Auth funktioniert trotzdem."
    }
fi

# 4. SSH Key kopieren (vom NUC oder manuell)
SSH_KEY="$HOME/.ssh/glinet_key"
if [ ! -f "$SSH_KEY" ]; then
    echo ""
    echo -e "${CYAN}SSH Key wird benoetigt fuer passwortlosen Router-Zugriff.${NC}"
    echo "Kopiere vom NUC: scp janpow@NUC_IP:~/.ssh/glinet_key* ~/.ssh/"
    echo "Oder generiere neu: ssh-keygen -t ed25519 -f ~/.ssh/glinet_key -N ''"
    echo "(Dann auf dem Router installieren: ssh-copy-id -i ~/.ssh/glinet_key root@192.168.8.1)"
    echo ""

    read -rp "NUC IP eingeben (oder Enter zum Ueberspringen): " NUC_IP
    if [ -n "$NUC_IP" ]; then
        scp "${NUC_IP}:~/.ssh/glinet_key" "$HOME/.ssh/glinet_key"
        scp "${NUC_IP}:~/.ssh/glinet_key.pub" "$HOME/.ssh/glinet_key.pub"
        chmod 600 "$HOME/.ssh/glinet_key"
        echo -e "${GREEN}SSH Key kopiert${NC}"
    fi
else
    echo -e "${GREEN}SSH Key: vorhanden${NC}"
fi

# 5. Projekt bauen
echo ""
echo "Baue VPN Router..."
cd "$PROJECT_DIR"
cargo build --release --bin vpn-router 2>&1 | tail -3

BINARY="$PROJECT_DIR/../../target/release/vpn-router"
if [ ! -f "$BINARY" ]; then
    # Standalone build (nicht im Workspace)
    BINARY="$PROJECT_DIR/target/release/vpn-router"
fi

echo ""
echo -e "${GREEN}=== Setup abgeschlossen ===${NC}"
echo ""
echo "Starten:"
echo "  cd $PROJECT_DIR"
echo "  ROUTER_IP=192.168.8.1 $BINARY"
echo ""
echo "Dann im Browser: http://localhost:3080"
echo ""
echo "Oder als LaunchAgent (Autostart):"
echo "  bash scripts/install-launchagent.sh"
