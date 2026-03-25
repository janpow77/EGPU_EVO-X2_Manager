#!/bin/bash
# deploy.sh
# Deployt die Web-UI auf den GL.iNet AXT1800 Router via SCP.

set -euo pipefail

ROUTER_IP="${ROUTER_IP:-192.168.8.1}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WEB_UI_DIR="${SCRIPT_DIR}/../web-ui"
REMOTE_PATH="/www/vpn-panel"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${GREEN}=== Deploy Web-UI auf GL.iNet Router ===${NC}"
echo -e "Router: ${CYAN}${ROUTER_IP}${NC}"
echo ""

# Pruefen ob Web-UI existiert
if [ ! -f "${WEB_UI_DIR}/index.html" ]; then
    echo -e "${RED}web-ui/index.html nicht gefunden!${NC}"
    exit 1
fi

# Router erreichbar?
echo -n "Pruefe Router... "
if ! ssh -o ConnectTimeout=3 -o BatchMode=yes "root@${ROUTER_IP}" "echo ok" 2>/dev/null; then
    echo -e "${YELLOW}SSH-Key nicht eingerichtet. Versuche mit Passwort...${NC}"
    echo "(Bei Erstnutzung: SSH aktivieren im Admin-Panel unter System -> SSH)"
fi

# Verzeichnis auf Router anlegen
echo "Erstelle Verzeichnis auf Router..."
ssh "root@${ROUTER_IP}" "mkdir -p ${REMOTE_PATH}"

# Dateien kopieren
echo "Kopiere Web-UI..."
scp "${WEB_UI_DIR}/index.html" "root@${ROUTER_IP}:${REMOTE_PATH}/index.html"

# Optional: zusaetzliche Dateien
for f in style.css app.js; do
    if [ -f "${WEB_UI_DIR}/${f}" ]; then
        scp "${WEB_UI_DIR}/${f}" "root@${ROUTER_IP}:${REMOTE_PATH}/${f}"
    fi
done

echo ""
echo -e "${GREEN}=== Deploy erfolgreich! ===${NC}"
echo ""
echo -e "Web-UI erreichbar unter: ${CYAN}http://${ROUTER_IP}/vpn-panel/${NC}"
echo ""
echo "Oeffne im Browser:"
echo "  http://${ROUTER_IP}/vpn-panel/"
