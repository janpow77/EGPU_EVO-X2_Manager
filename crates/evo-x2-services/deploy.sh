#!/usr/bin/env bash
set -euo pipefail

# deploy.sh — Baut evo-x2-services und deployed auf die EVO-X2
# Aufruf: bash deploy.sh [IP] [USER]

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN="$WORKSPACE_ROOT/target/release/evo-x2-services"

# Config oder Parameter
CONFIG="$HOME/.config/evo-manager/config.json"
if [[ -f "$CONFIG" ]]; then
    EVO_IP="${1:-$(python3 -c "import json; print(json.load(open('$CONFIG')).get('evo_ip',''))" 2>/dev/null || echo "")}"
    EVO_USER="${2:-$(python3 -c "import json; print(json.load(open('$CONFIG')).get('ssh_user','jan'))" 2>/dev/null || echo "jan")}"
else
    EVO_IP="${1:-}"
    EVO_USER="${2:-jan}"
fi

if [[ -z "$EVO_IP" ]]; then
    echo "Verwendung: bash deploy.sh <EVO-X2-IP> [USER]"
    echo "Oder: evo_ip in ~/.config/evo-manager/config.json setzen"
    exit 1
fi

echo "=== EVO-X2 Services Deploy ==="
echo "  Ziel: $EVO_USER@$EVO_IP"

# Build
echo "Baue Release..."
cd "$WORKSPACE_ROOT"
cargo build --release -p evo-x2-services

echo "Kopiere Binary auf EVO-X2..."
scp -o ConnectTimeout=5 "$BIN" "$EVO_USER@$EVO_IP:~/.local/bin/evo-x2-services"

echo "Restarte Services..."
ssh -o ConnectTimeout=5 "$EVO_USER@$EVO_IP" \
    "sudo systemctl restart evo-metrics evo-webhook"

echo "Health-Checks..."
for port in 8084 9000 8083; do
    if ssh -o ConnectTimeout=5 "$EVO_USER@$EVO_IP" "curl -sf http://localhost:$port/health" >/dev/null 2>&1; then
        echo "  Port $port: OK"
    else
        echo "  Port $port: FEHLER"
    fi
done

echo ""
echo "Deploy abgeschlossen."
