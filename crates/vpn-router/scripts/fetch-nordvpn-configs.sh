#!/bin/bash
# fetch-nordvpn-configs.sh
# Laedt NordVPN WireGuard-Konfigurationen fuer deutsche Server herunter.
# Voraussetzung: NordVPN Access Token (https://my.nordaccount.com -> Manual Setup)

set -euo pipefail

CONFIGS_DIR="$(cd "$(dirname "$0")/../configs" && pwd)"
TOKEN_FILE="$(cd "$(dirname "$0")/.." && pwd)/.nordvpn-token"

# Farben
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}=== NordVPN WireGuard Config Downloader ===${NC}"
echo ""

# Token laden oder abfragen
if [ -f "$TOKEN_FILE" ]; then
    TOKEN=$(cat "$TOKEN_FILE" | tr -d '[:space:]')
    echo -e "Token aus ${YELLOW}$TOKEN_FILE${NC} geladen."
else
    echo -e "${YELLOW}Kein Token gefunden.${NC}"
    echo "Erstelle einen unter: https://my.nordaccount.com -> Services -> NordVPN -> Manual Setup"
    echo ""
    read -rp "NordVPN Access Token eingeben: " TOKEN
    if [ -z "$TOKEN" ]; then
        echo -e "${RED}Kein Token eingegeben. Abbruch.${NC}"
        exit 1
    fi
    echo "$TOKEN" > "$TOKEN_FILE"
    chmod 600 "$TOKEN_FILE"
    echo -e "Token gespeichert in ${YELLOW}$TOKEN_FILE${NC}"
fi

echo ""
echo "Lade Credentials von NordVPN API..."

# NordVPN WireGuard Credentials holen
CREDS=$(curl -s -u "token:${TOKEN}" "https://api.nordvpn.com/v1/users/services/credentials" 2>/dev/null) || {
    echo -e "${RED}Fehler beim Abrufen der Credentials. Token ungueltig?${NC}"
    exit 1
}

PRIVATE_KEY=$(echo "$CREDS" | python3 -c "import sys,json; print(json.load(sys.stdin)['nordlynx_private_key'])" 2>/dev/null) || {
    echo -e "${RED}Konnte Private Key nicht extrahieren. API-Antwort pruefen.${NC}"
    echo "$CREDS" | head -c 200
    exit 1
}

echo -e "${GREEN}Private Key erhalten.${NC}"
echo ""

# Deutsche Server holen (recommended)
echo "Suche beste deutsche NordVPN-Server..."
SERVERS=$(curl -s "https://api.nordvpn.com/v1/servers/recommendations?filters\[country_id\]=81&filters\[servers_technologies\][identifier]=wireguard_udp&limit=8" 2>/dev/null) || {
    echo -e "${RED}Fehler beim Abrufen der Server-Liste.${NC}"
    exit 1
}

# Server parsen und Configs erstellen
echo "$SERVERS" | python3 -c "
import sys, json

data = json.load(sys.stdin)
if not data:
    print('Keine deutschen Server gefunden.')
    sys.exit(1)

servers = []
for s in data:
    name = s.get('name', 'unknown')
    hostname = s.get('hostname', '')
    city = ''
    if 'locations' in s and s['locations']:
        loc = s['locations'][0]
        if 'country' in loc and 'city' in loc.get('country', {}):
            city = loc['country']['city'].get('name', '')
        elif 'city' in loc:
            city = loc['city'].get('name', '')

    # WireGuard public key und IP finden
    wg_tech = None
    for tech in s.get('technologies', []):
        if tech.get('identifier') == 'wireguard_udp':
            wg_tech = tech
            break

    if not wg_tech:
        continue

    pub_key = ''
    for meta in wg_tech.get('metadata', []):
        if meta.get('name') == 'public_key':
            pub_key = meta.get('value', '')

    ip = s.get('station', s.get('ips', [{}])[0].get('ip', {}).get('ip', ''))
    if not ip and 'ips' in s:
        for ip_entry in s['ips']:
            if ip_entry.get('type', {}).get('identifier') == 'wireguard_udp':
                ip = ip_entry['ip']['ip']
                break
        if not ip and s['ips']:
            ip = s['ips'][0]['ip']['ip']

    if pub_key and ip:
        servers.append({
            'name': name,
            'hostname': hostname,
            'city': city,
            'ip': ip,
            'public_key': pub_key
        })

# JSON ausgeben fuer das Shell-Script
json.dump(servers, sys.stdout)
" > /tmp/nordvpn_de_servers.json 2>/dev/null

SERVER_COUNT=$(python3 -c "import json; print(len(json.load(open('/tmp/nordvpn_de_servers.json'))))" 2>/dev/null || echo "0")

if [ "$SERVER_COUNT" -eq "0" ]; then
    echo -e "${RED}Keine Server gefunden. API-Format hat sich moeglicherweise geaendert.${NC}"
    exit 1
fi

echo -e "${GREEN}${SERVER_COUNT} deutsche Server gefunden.${NC}"
echo ""

# WireGuard-Configs erstellen
python3 -c "
import json, os, sys

servers = json.load(open('/tmp/nordvpn_de_servers.json'))
private_key = '${PRIVATE_KEY}'
configs_dir = '${CONFIGS_DIR}'
created = []

# Maximal 4 Server (verschiedene Staedte bevorzugen)
seen_cities = set()
selected = []
for s in servers:
    city = s.get('city', 'unknown').lower()
    if city not in seen_cities:
        selected.append(s)
        seen_cities.add(city)
    if len(selected) >= 4:
        break

# Falls weniger als 4 Staedte, weitere Server auffuellen
if len(selected) < 4:
    for s in servers:
        if s not in selected:
            selected.append(s)
        if len(selected) >= 4:
            break

for s in selected:
    city = s.get('city', 'unknown').replace(' ', '-').lower()
    hostname = s.get('hostname', 'unknown')
    safe_name = f'nordvpn-de-{city}'

    config = f'''[Interface]
PrivateKey = {private_key}
Address = 10.5.0.2/16
DNS = 103.86.96.100, 103.86.99.100

[Peer]
PublicKey = {s['public_key']}
AllowedIPs = 0.0.0.0/0, ::/0
Endpoint = {s['ip']}:51820
PersistentKeepalive = 25
'''

    filepath = os.path.join(configs_dir, f'{safe_name}.conf')
    with open(filepath, 'w') as f:
        f.write(config)
    os.chmod(filepath, 0o600)
    created.append((safe_name, s.get('city', '?'), s.get('hostname', '?')))
    print(f'  Erstellt: {safe_name}.conf  ({s.get(\"city\", \"?\")} - {hostname})')

print()
print(f'{len(created)} WireGuard-Configs erstellt in {configs_dir}/')
"

# Aufraeumen
rm -f /tmp/nordvpn_de_servers.json

echo ""
echo -e "${GREEN}=== Fertig! ===${NC}"
echo ""
echo "Naechster Schritt:"
echo "  1. Configs pruefen:  ls -la $CONFIGS_DIR/"
echo "  2. Router einrichten: ./scripts/configure-router.sh"
echo "  3. Oder Web-UI nutzen: ./scripts/deploy.sh"
