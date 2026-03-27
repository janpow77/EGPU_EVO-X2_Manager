#!/usr/bin/env bash
# Ollama KEEP_ALIVE auf unbegrenzt setzen (Modelle bleiben immer geladen)
ssh -t janpow@192.168.178.72 \
  "sudo mv /tmp/ollama-override.conf /etc/systemd/system/ollama.service.d/override.conf && sudo systemctl daemon-reload && sudo systemctl restart ollama"
sleep 5
echo "--- Ollama Config ---"
ssh janpow@192.168.178.72 'cat /etc/systemd/system/ollama.service.d/override.conf'
echo "--- Laufende Modelle ---"
ssh janpow@192.168.178.72 'ollama ps'
