#!/usr/bin/env bash
# Ollama auf EVO-X2 neustarten und GPU-Erkennung prüfen
ssh -t janpow@192.168.178.72 'sudo systemctl restart ollama'
sleep 5
echo "--- GPU-Erkennung ---"
ssh janpow@192.168.178.72 'journalctl -u ollama --no-pager --since "30 sec ago" | grep -i "inference compute"'
echo "--- Modell-Test ---"
curl -sf --max-time 120 "http://192.168.178.72:11434/api/chat" -d '{"model":"mannix/dolphin-2.9.2-qwen2-72b:q4_k_m","messages":[{"role":"user","content":"Say OK"}],"stream":false,"options":{"num_predict":5}}' | python3 -c "import sys,json; print(json.load(sys.stdin)['message']['content'])"
sleep 2
echo "--- GPU-Auslastung ---"
ssh janpow@192.168.178.72 'ollama ps'
