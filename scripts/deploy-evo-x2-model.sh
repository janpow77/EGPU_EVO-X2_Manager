#!/bin/bash
# Deploy-Script fuer EVO X2: Modellwechsel + Ollama-Konfiguration
# Ausfuehren auf dem NUC — verbindet sich per SSH zur EVO X2.
set -euo pipefail

EVO_IP="${EVO_X2_IP:-100.81.4.99}"
EVO_USER="${EVO_X2_USER:-janpow}"
NEW_MODEL="qwen3:32b"
OLD_MODEL="qwen2.5:72b-instruct-q4_K_M"

echo "============================================="
echo "  EVO X2 Modell-Deploy: ${OLD_MODEL} → ${NEW_MODEL}"
echo "  Ziel: ${EVO_USER}@${EVO_IP}"
echo "============================================="
echo ""

# SSH-Verbindung testen
echo "[1/5] SSH-Verbindung pruefen..."
if ! ssh -o ConnectTimeout=5 -o BatchMode=yes "${EVO_USER}@${EVO_IP}" "echo ok" 2>/dev/null; then
    echo "  FEHLER: SSH-Key nicht konfiguriert. Bitte erst:"
    echo "    ssh-copy-id ${EVO_USER}@${EVO_IP}"
    exit 1
fi
echo "  OK"
SSH="ssh -t ${EVO_USER}@${EVO_IP}"

# Ollama NUM_PARALLEL konfigurieren
echo ""
echo "[2/5] Ollama NUM_PARALLEL=2 konfigurieren..."
${SSH} "sudo mkdir -p /etc/systemd/system/ollama.service.d && echo '[Service]
Environment=\"OLLAMA_NUM_PARALLEL=2\"
Environment=\"OLLAMA_MAX_QUEUE=8\"' | sudo tee /etc/systemd/system/ollama.service.d/parallel.conf && sudo systemctl daemon-reload && echo 'OK: NUM_PARALLEL=2, MAX_QUEUE=8'"

# Neues Modell pullen
echo ""
echo "[3/5] Modell ${NEW_MODEL} herunterladen (kann einige Minuten dauern)..."
${SSH} "ollama pull ${NEW_MODEL}"

# Ollama neustarten
echo ""
echo "[4/5] Ollama neustarten..."
${SSH} "sudo systemctl restart ollama"
echo "  Warte 5s auf Start..."
sleep 5

# Verifizierung
echo ""
echo "[5/5] Verifizierung..."
echo ""

echo "--- Ollama-Status ---"
curl -s --max-time 10 "http://${EVO_IP}:11434/api/ps" 2>/dev/null | python3 -c "
import sys, json
d = json.load(sys.stdin)
models = d.get('models', [])
if models:
    for m in models:
        vram = m['size_vram']/1024/1024/1024
        print(f'  {m[\"name\"]:50s} VRAM={vram:.1f} GB')
else:
    print('  (keine Modelle geladen — werden beim ersten Request geladen)')
" 2>/dev/null || echo "  Ollama noch nicht bereit"

echo ""
echo "--- Verfuegbare Modelle ---"
curl -s --max-time 5 "http://${EVO_IP}:11434/api/tags" 2>/dev/null | python3 -c "
import sys, json
d = json.load(sys.stdin)
for m in d.get('models', []):
    size_gb = m['size']/1024/1024/1024
    print(f'  {m[\"name\"]:50s} {size_gb:.1f} GB')
" 2>/dev/null || echo "  Nicht erreichbar"

echo ""
echo "--- Kurztest ${NEW_MODEL} ---"
RESULT=$(curl -s --max-time 120 "http://${EVO_IP}:11434/v1/chat/completions" \
  -H 'Content-Type: application/json' \
  -d "{\"model\":\"${NEW_MODEL}\",\"messages\":[{\"role\":\"user\",\"content\":\"Sag OK\"}],\"stream\":false,\"max_tokens\":5}" 2>/dev/null)

if echo "$RESULT" | python3 -c "import sys,json;d=json.load(sys.stdin);print(f'  Antwort: {d[\"choices\"][0][\"message\"][\"content\"]}')" 2>/dev/null; then
    echo "  OK — ${NEW_MODEL} funktioniert!"
else
    echo "  WARNUNG: ${NEW_MODEL} antwortet noch nicht (Cold-Start, nochmal versuchen)"
fi

echo ""
echo "============================================="
echo "  Deploy abgeschlossen."
echo ""
echo "  Naechste Schritte auf dem NUC:"
echo "    cd ~/Projekte/EGPU_EVO-X2_Manager"
echo "    sudo bash deploy.sh"
echo "============================================="
