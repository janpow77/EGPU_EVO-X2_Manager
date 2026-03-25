#!/bin/bash
# emergency-restore.sh
# Stellt den Router nach Factory Reset komplett wieder her.
# Ausfuehren: ./scripts/emergency-restore.sh
#
# Voraussetzung:
#   - Router frisch resetted, per WiFi verbunden (GL-AXT1800-xxx, PW: S4TSBEXFYH)
#   - NUC per WiFi im Router-Netz (192.168.8.x)
#   - sshpass installiert

set -euo pipefail

ROUTER_IP="${ROUTER_IP:-192.168.8.1}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CONFIGS_DIR="${SCRIPT_DIR}/../configs"
SSH_KEY="${HOME}/.ssh/glinet_key"
NEW_PASSWORD="VpnRouter2024!"
WIFI_SSID="JP-Travel"
WIFI_PASSWORD="BelgradVPN2024!"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

ssh_cmd() {
    if [ -f "$SSH_KEY" ]; then
        ssh -i "$SSH_KEY" -o StrictHostKeyChecking=no -o ConnectTimeout=5 "root@${ROUTER_IP}" "$1" 2>/dev/null && return
    fi
    sshpass -p "$NEW_PASSWORD" ssh -o StrictHostKeyChecking=no -o ConnectTimeout=5 "root@${ROUTER_IP}" "$1" 2>/dev/null && return
    sshpass -p '' ssh -o StrictHostKeyChecking=no -o ConnectTimeout=5 "root@${ROUTER_IP}" "$1" 2>/dev/null
}

echo -e "${GREEN}=== Emergency Router Restore ===${NC}"
echo -e "Router: ${CYAN}${ROUTER_IP}${NC}"
echo ""

# ── Step 1: Warte auf Router ──
echo -n "Warte auf Router... "
for i in $(seq 1 30); do
    if ping -c 1 -W 1 "$ROUTER_IP" &>/dev/null; then
        echo -e "${GREEN}erreichbar!${NC}"
        break
    fi
    echo -n "."
    sleep 2
done

# ── Step 2: Passwort setzen ──
echo -e "\n${CYAN}--- Schritt 1/6: Passwort setzen ---${NC}"
sshpass -p '' ssh -o StrictHostKeyChecking=no "root@${ROUTER_IP}" \
    "echo -e '${NEW_PASSWORD}\n${NEW_PASSWORD}' | passwd root" 2>/dev/null && \
    echo -e "${GREEN}Passwort gesetzt: ${NEW_PASSWORD}${NC}" || \
    echo -e "${YELLOW}Passwort war bereits gesetzt${NC}"

# ── Step 3: SSH Key installieren ──
echo -e "\n${CYAN}--- Schritt 2/6: SSH Key ---${NC}"
if [ -f "${SSH_KEY}.pub" ]; then
    cat "${SSH_KEY}.pub" | sshpass -p "$NEW_PASSWORD" ssh -o StrictHostKeyChecking=no "root@${ROUTER_IP}" \
        'mkdir -p /etc/dropbear; cat >> /etc/dropbear/authorized_keys; chmod 600 /etc/dropbear/authorized_keys' 2>/dev/null
    echo -e "${GREEN}SSH Key installiert${NC}"
else
    echo -e "${YELLOW}Kein SSH Key gefunden - nutze Passwort${NC}"
fi

# ── Step 4: Captive Portal deaktivieren ──
echo -e "\n${CYAN}--- Schritt 3/6: Captive Portal deaktivieren ---${NC}"
ssh_cmd "
uci set glconfig.general.init_status='1' 2>/dev/null
uci set glconfig.general.init_pwd='1' 2>/dev/null
uci commit glconfig 2>/dev/null

# Remove redirect rules
for name in capture22 capture443 captureport; do
    section=\$(uci show firewall 2>/dev/null | grep \"name='\$name'\" | cut -d. -f2 | cut -d= -f1)
    [ -n \"\$section\" ] && uci delete firewall.\$section
done
uci commit firewall
echo DONE
"
echo -e "${GREEN}Captive Portal deaktiviert${NC}"

# ── Step 5: WiFi AP konfigurieren ──
echo -e "\n${CYAN}--- Schritt 4/6: WiFi AP '${WIFI_SSID}' ---${NC}"
ssh_cmd "
# Set SSID and password for all AP interfaces
for iface in \$(uci show wireless | grep \"mode='ap'\" | cut -d. -f2 | cut -d. -f1); do
    uci set wireless.\$iface.ssid='${WIFI_SSID}'
    uci set wireless.\$iface.key='${WIFI_PASSWORD}'
    uci set wireless.\$iface.encryption='psk2+ccmp'
done
uci commit wireless
wifi reload
echo DONE
"
echo -e "${GREEN}WiFi: ${WIFI_SSID} / ${WIFI_PASSWORD}${NC}"

# ── Step 6: WireGuard VPN einrichten ──
echo -e "\n${CYAN}--- Schritt 5/6: WireGuard VPN ---${NC}"
if [ -f "${CONFIGS_DIR}/wireguard-uci.txt" ]; then
    # Restore from saved UCI config
    while IFS= read -r line; do
        key=$(echo "$line" | cut -d= -f1)
        val=$(echo "$line" | cut -d= -f2- | sed "s/^'//;s/'$//")
        ssh_cmd "uci set ${key}='${val}'" 2>/dev/null
    done < "${CONFIGS_DIR}/wireguard-uci.txt"
    ssh_cmd "uci commit network"
    echo -e "${GREEN}WireGuard Config wiederhergestellt${NC}"
else
    echo -e "${YELLOW}Keine gespeicherte WireGuard Config - nutze Web-UI zum Einrichten${NC}"
fi

# ── Step 7: DNS + Sicherheit ──
echo -e "\n${CYAN}--- Schritt 6/6: DNS + Firewall ---${NC}"
ssh_cmd "
# DNS: NordVPN Server
uci set dhcp.@dnsmasq[0].noresolv='1'
uci delete dhcp.@dnsmasq[0].server 2>/dev/null
uci add_list dhcp.@dnsmasq[0].server='103.86.96.100'
uci add_list dhcp.@dnsmasq[0].server='103.86.99.100'
uci set dhcp.@dnsmasq[0].rebind_protection='0'
uci commit dhcp
/etc/init.d/dnsmasq restart

# Firewall: VPN Zone
uci set firewall.vpn_zone=zone
uci set firewall.vpn_zone.name='vpn'
uci set firewall.vpn_zone.input='REJECT'
uci set firewall.vpn_zone.output='ACCEPT'
uci set firewall.vpn_zone.forward='REJECT'
uci set firewall.vpn_zone.masq='1'
uci set firewall.vpn_zone.mtu_fix='1'
uci set firewall.vpn_zone.network='wg_de_frankfurt'

uci set firewall.vpn_fwd=forwarding
uci set firewall.vpn_fwd.src='lan'
uci set firewall.vpn_fwd.dest='vpn'

uci commit firewall
/etc/init.d/firewall reload 2>/dev/null
/etc/init.d/network reload

echo DONE
"
echo -e "${GREEN}DNS + Firewall konfiguriert${NC}"

# ── Step 8: VPN starten ──
echo -e "\n${CYAN}--- VPN starten ---${NC}"
ssh_cmd "ifup wg_de_frankfurt; sleep 3; wg show 2>/dev/null | head -5"

echo ""
echo -e "${GREEN}=== Restore abgeschlossen! ===${NC}"
echo ""
echo "  Admin-PW:    ${NEW_PASSWORD}"
echo "  WiFi SSID:   ${WIFI_SSID}"
echo "  WiFi PW:     ${WIFI_PASSWORD}"
echo "  VPN:         DE-Frankfurt"
echo ""
echo "  Dienstgeraete mit '${WIFI_SSID}' verbinden!"
echo ""
echo "  Web-UI starten: ROUTER_IP=${ROUTER_IP} ./target/debug/vpn-router"
