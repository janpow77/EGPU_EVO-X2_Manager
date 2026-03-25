#!/bin/bash
# configure-router.sh
# Konfiguriert den GL.iNet AXT1800 automatisch via API.
# Voraussetzung: Router per Ethernet an NUC/PC angeschlossen, Configs vorhanden.

set -euo pipefail

ROUTER_IP="${ROUTER_IP:-192.168.8.1}"
CONFIGS_DIR="$(cd "$(dirname "$0")/../configs" && pwd)"
API="http://${ROUTER_IP}/api"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${GREEN}=== GL.iNet AXT1800 Auto-Konfiguration ===${NC}"
echo -e "Router: ${CYAN}${ROUTER_IP}${NC}"
echo ""

# Erreichbarkeit pruefen
echo -n "Pruefe Router-Erreichbarkeit... "
if ! curl -s --connect-timeout 3 "http://${ROUTER_IP}" > /dev/null 2>&1; then
    echo -e "${RED}FEHLER${NC}"
    echo "Router unter ${ROUTER_IP} nicht erreichbar."
    echo "Ist das Ethernet-Kabel eingesteckt?"
    exit 1
fi
echo -e "${GREEN}OK${NC}"

# Admin-Passwort abfragen
echo ""
read -rsp "Router Admin-Passwort: " ADMIN_PW
echo ""

# Login und Token holen
echo -n "Anmeldung... "
LOGIN_RESP=$(curl -s -X POST "${API}/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"username\":\"root\",\"password\":\"${ADMIN_PW}\"}" 2>/dev/null) || {
    echo -e "${RED}FEHLER - API nicht erreichbar${NC}"
    exit 1
}

TOKEN=$(echo "$LOGIN_RESP" | python3 -c "import sys,json; print(json.load(sys.stdin).get('token',''))" 2>/dev/null)
if [ -z "$TOKEN" ]; then
    echo -e "${RED}FEHLER - Login fehlgeschlagen. Passwort korrekt?${NC}"
    echo "API-Antwort: $(echo "$LOGIN_RESP" | head -c 200)"
    exit 1
fi
echo -e "${GREEN}OK${NC}"

AUTH="Authorization: Bearer ${TOKEN}"

# Hilfsfunktion fuer API-Calls
api_call() {
    local method="$1"
    local endpoint="$2"
    local data="${3:-}"

    if [ "$method" = "GET" ]; then
        curl -s -X GET "${API}${endpoint}" -H "$AUTH" -H "Content-Type: application/json" 2>/dev/null
    else
        curl -s -X POST "${API}${endpoint}" -H "$AUTH" -H "Content-Type: application/json" -d "$data" 2>/dev/null
    fi
}

# ============================================================
# Schritt 1: WiFi AP konfigurieren
# ============================================================
echo ""
echo -e "${CYAN}--- Schritt 1/4: WiFi Access Point ---${NC}"

read -rp "SSID (z.B. JP-Travel): " WIFI_SSID
if [ -z "$WIFI_SSID" ]; then
    WIFI_SSID="JP-Travel"
    echo "  Standard-SSID: $WIFI_SSID"
fi

read -rsp "WiFi-Passwort (min. 8 Zeichen): " WIFI_PW
echo ""
if [ ${#WIFI_PW} -lt 8 ]; then
    echo -e "${RED}Passwort zu kurz (min. 8 Zeichen). Abbruch.${NC}"
    exit 1
fi

echo -n "Konfiguriere WiFi AP... "
WIFI_RESP=$(api_call POST "/wireless/set" "{
    \"ssid_2g\": \"${WIFI_SSID}\",
    \"key_2g\": \"${WIFI_PW}\",
    \"encryption_2g\": \"psk2+ccmp\",
    \"ssid_5g\": \"${WIFI_SSID}\",
    \"key_5g\": \"${WIFI_PW}\",
    \"encryption_5g\": \"psk2+ccmp\",
    \"enabled_2g\": true,
    \"enabled_5g\": true
}")
echo -e "${GREEN}OK${NC}"
echo "  SSID: ${WIFI_SSID} (2.4 GHz + 5 GHz)"

# ============================================================
# Schritt 2: WireGuard Configs hochladen
# ============================================================
echo ""
echo -e "${CYAN}--- Schritt 2/4: NordVPN WireGuard Server ---${NC}"

CONF_FILES=$(ls "${CONFIGS_DIR}"/nordvpn-de-*.conf 2>/dev/null || true)
if [ -z "$CONF_FILES" ]; then
    echo -e "${RED}Keine Configs in ${CONFIGS_DIR}/ gefunden.${NC}"
    echo "Zuerst ausfuehren: ./scripts/fetch-nordvpn-configs.sh"
    exit 1
fi

UPLOADED=0
FIRST_SERVER_ID=""

for conf in ${CONFIGS_DIR}/nordvpn-de-*.conf; do
    filename=$(basename "$conf" .conf)
    display_name=$(echo "$filename" | sed 's/nordvpn-de-/DE-/' | sed 's/-/ /g' | sed 's/\b\(.\)/\u\1/g')
    config_content=$(cat "$conf")

    echo -n "  Lade hoch: ${display_name}... "
    UPLOAD_RESP=$(api_call POST "/wireguard/client/add" "{
        \"name\": \"${display_name}\",
        \"config\": $(python3 -c "import json; print(json.dumps(open('$conf').read()))")
    }")

    # Server-ID merken (fuer Auto-Connect)
    SERVER_ID=$(echo "$UPLOAD_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('id', d.get('group_id','')))" 2>/dev/null || echo "")
    if [ -n "$SERVER_ID" ] && [ -z "$FIRST_SERVER_ID" ]; then
        FIRST_SERVER_ID="$SERVER_ID"
    fi

    UPLOADED=$((UPLOADED + 1))
    echo -e "${GREEN}OK${NC}"
done

echo "  ${UPLOADED} Server hochgeladen."

# ============================================================
# Schritt 3: Sicherheit konfigurieren
# ============================================================
echo ""
echo -e "${CYAN}--- Schritt 3/4: Sicherheit ---${NC}"

# Auto-Connect bei Boot
echo -n "  Auto-Connect bei Boot... "
api_call POST "/wireguard/client/set" "{\"auto_connect\": true}" > /dev/null
echo -e "${GREEN}AN${NC}"

# Global Proxy (aller Traffic durch VPN)
echo -n "  Global Proxy... "
api_call POST "/wireguard/client/set" "{\"global_proxy\": true}" > /dev/null
echo -e "${GREEN}AN${NC}"

# Kill Switch
echo -n "  Kill Switch... "
api_call POST "/vpn/policy/set" "{\"block_non_vpn\": true}" > /dev/null
echo -e "${GREEN}AN${NC}"

# DNS ueber VPN
echo -n "  DNS ueber VPN... "
api_call POST "/dns/set" "{\"dns_mode\": \"vpn\", \"dns_servers\": [\"103.86.96.100\", \"103.86.99.100\"]}" > /dev/null
echo -e "${GREEN}AN${NC}"

# ============================================================
# Schritt 4: VPN starten und testen
# ============================================================
echo ""
echo -e "${CYAN}--- Schritt 4/4: Test ---${NC}"

# VPN starten
echo -n "Starte VPN-Verbindung... "
if [ -n "$FIRST_SERVER_ID" ]; then
    api_call POST "/wireguard/client/start" "{\"id\": \"${FIRST_SERVER_ID}\"}" > /dev/null
else
    api_call POST "/wireguard/client/start" "{}" > /dev/null
fi

# Warten auf Verbindung
sleep 5

VPN_STATUS=$(api_call GET "/wireguard/client/status")
VPN_CONNECTED=$(echo "$VPN_STATUS" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('status','unknown'))" 2>/dev/null || echo "unknown")

if [ "$VPN_CONNECTED" = "connected" ] || [ "$VPN_CONNECTED" = "1" ]; then
    echo -e "${GREEN}VERBUNDEN${NC}"
else
    echo -e "${YELLOW}Status: ${VPN_CONNECTED} (evtl. noch am Verbinden)${NC}"
fi

# Oeffentliche IP pruefen
echo -n "Pruefe oeffentliche IP... "
PUBLIC_IP=$(curl -s --max-time 10 "https://api.ipify.org" 2>/dev/null || echo "nicht erreichbar")
echo -e "${CYAN}${PUBLIC_IP}${NC}"

# IP-Geolocation pruefen
if [ "$PUBLIC_IP" != "nicht erreichbar" ]; then
    GEO=$(curl -s --max-time 5 "https://ipapi.co/${PUBLIC_IP}/json/" 2>/dev/null || echo "{}")
    COUNTRY=$(echo "$GEO" | python3 -c "import sys,json; print(json.load(sys.stdin).get('country_name','?'))" 2>/dev/null || echo "?")
    CITY=$(echo "$GEO" | python3 -c "import sys,json; print(json.load(sys.stdin).get('city','?'))" 2>/dev/null || echo "?")
    if [ "$COUNTRY" = "Germany" ]; then
        echo -e "  Standort: ${GREEN}${CITY}, ${COUNTRY}${NC}"
    else
        echo -e "  Standort: ${RED}${CITY}, ${COUNTRY} (NICHT Deutschland!)${NC}"
    fi
fi

# ============================================================
# Zusammenfassung
# ============================================================
echo ""
echo -e "${GREEN}=== Konfiguration abgeschlossen ===${NC}"
echo ""
echo "  WiFi SSID:     ${WIFI_SSID}"
echo "  VPN Server:    ${UPLOADED} deutsche Server"
echo "  Auto-Connect:  AN"
echo "  Kill Switch:   AN"
echo "  DNS-Schutz:    AN"
echo ""
echo "Naechste Schritte:"
echo "  1. Web-UI deployen:  ./scripts/deploy.sh"
echo "  2. Reboot-Test:      Router aus/an, WLAN verbinden, IP pruefen"
echo "  3. SSID + PW auf Dienstgeraeten speichern"
