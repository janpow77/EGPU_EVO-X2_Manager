#!/bin/bash
# Startet Ollama auf der EVO X2 neu.
# Methode 1: HTTP-Endpoint (wenn evo-metrics laeuft)
# Methode 2: SSH-Fallback
set -euo pipefail

EVO_IP="${EVO_X2_IP:-100.81.4.99}"
EVO_USER="${EVO_X2_USER:-jan}"
METRICS_PORT="${EVO_METRICS_PORT:-8084}"

echo "=== Ollama Restart auf EVO X2 ($EVO_IP) ==="

# Methode 1: HTTP-Endpoint (bevorzugt, kein SSH noetig)
echo "[1] Versuche HTTP-Restart via evo-metrics..."
RESULT=$(curl -s --max-time 15 -X POST "http://${EVO_IP}:${METRICS_PORT}/restart-ollama" 2>&1) && {
    echo "    OK: $RESULT"
    echo ""
    echo "Warte 5s auf Ollama-Start..."
    sleep 5
    echo "Status:"
    curl -s --max-time 5 "http://${EVO_IP}:11434/api/ps" 2>/dev/null | python3 -m json.tool 2>/dev/null || echo "    (noch nicht bereit)"
    exit 0
}

echo "    HTTP fehlgeschlagen, versuche SSH..."

# Methode 2: SSH
echo "[2] Versuche SSH-Restart..."
ssh -o ConnectTimeout=5 -o StrictHostKeyChecking=no "${EVO_USER}@${EVO_IP}" \
    "sudo systemctl restart ollama && echo 'Ollama neugestartet'" && {
    echo ""
    echo "Warte 5s auf Ollama-Start..."
    sleep 5
    echo "Status:"
    curl -s --max-time 5 "http://${EVO_IP}:11434/api/ps" 2>/dev/null | python3 -m json.tool 2>/dev/null || echo "    (noch nicht bereit)"
    exit 0
}

echo ""
echo "FEHLER: Weder HTTP noch SSH haben funktioniert."
echo "Bitte manuell auf der EVO X2: sudo systemctl restart ollama"
exit 1
