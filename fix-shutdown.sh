#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# fix-shutdown.sh — Sauberer Shutdown + Auto-Recovery für EVO-X2
# ============================================================================
#
# Löst folgende Szenarien:
#
# 1. NORMALER SHUTDOWN (sudo poweroff / sudo reboot)
#    → Ollama bekommt SIGTERM, hat 30s zum Modell-Entladen
#    → Services stoppen sauber
#
# 2. HARD POWER-CYCLE (Shelly / Stecker ziehen)
#    → Strom weg, kein sauberer Shutdown möglich
#    → Beim nächsten Boot: ext4-Journal-Recovery (automatisch)
#    → Ollama Blob-Store ist crash-safe (nur Reads, keine DB-Writes)
#    → ollama-preload.service lädt alle Modelle automatisch
#
# 3. KERNEL-PANIC
#    → kernel.panic=10: nach 10s automatischer Reboot
#    → Danach wie Szenario 2
#
# 4. OLLAMA-CRASH (Segfault, OOM)
#    → systemd Restart=always, RestartSec=3
#    → Modelle müssen manuell angefragt werden (KEEP_ALIVE=-1 hält sie dann)
#    → ollama-health-check.timer prüft alle 60s und preloaded bei Bedarf
#
# Was NICHT fixbar ist ohne BIOS-Zugang:
#    → "Power On After Power Loss" muss im BIOS aktiviert sein
#      (GMKtec EVO-X2: BIOS → Advanced → Power On After Power Fail → [Enabled])
#      Ohne diese Einstellung bleibt die EVO-X2 nach Shelly-Cycle aus!

EVO_HOST="janpow@192.168.178.72"
SSH_OPTS="-o ConnectTimeout=10"

echo "============================================"
echo "  EVO-X2 Shutdown & Recovery Fix"
echo "============================================"
echo ""

# ---------------------------------------------------------------------------
# 1. Dateien vorbereiten (lokal in /tmp)
# ---------------------------------------------------------------------------

# Ollama Override: Graceful Shutdown + OOM-Schutz
cat > /tmp/evo-ollama-override.conf << 'EOF'
[Service]
Environment=OLLAMA_HOST=0.0.0.0
Environment=OLLAMA_KEEP_ALIVE=-1
# Graceful Shutdown: 30s zum Entladen der Modelle aus GTT
TimeoutStopSec=30
# OOM-Killer: Ollama ist wichtiger als andere Prozesse
OOMScoreAdjust=-500
EOF

# Kernel Panic Auto-Reboot + vm.panic_on_oom
cat > /tmp/evo-99-recovery.conf << 'EOF'
# Bei Kernel-Panic nach 10s automatisch rebooten
kernel.panic = 10
kernel.panic_on_oops = 1
# Bei OOM: Kernel-Log schreiben, nicht sofort panicen
vm.panic_on_oom = 0
vm.oom_kill_allocating_task = 1
EOF

# Modell-Preload Service (nach jedem Boot)
cat > /tmp/evo-ollama-preload.service << 'EOF'
[Unit]
Description=Ollama Modell-Preload (32B + 72B + bge-m3)
After=ollama.service network-online.target
Requires=ollama.service
Wants=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
# Warte bis Ollama API bereit (max 60s)
ExecStartPre=/bin/bash -c 'for i in $(seq 1 60); do curl -sf http://localhost:11434/api/tags >/dev/null 2>&1 && exit 0; sleep 1; done; echo "Ollama nicht bereit nach 60s"; exit 1'
# Lade alle Modelle (sequentiell — parallel kann GTT-Allokation fehlschlagen)
ExecStart=/bin/bash -c '\
  echo "Lade bge-m3..." && \
  curl -sf --max-time 60 http://localhost:11434/api/embed \
    -d "{\"model\":\"bge-m3\",\"input\":\"warmup\"}" >/dev/null 2>&1 && \
  echo "Lade abliterate-32b..." && \
  curl -sf --max-time 300 http://localhost:11434/api/chat \
    -d "{\"model\":\"huihui_ai/qwen2.5-abliterate:32b-instruct\",\"messages\":[{\"role\":\"user\",\"content\":\"warmup\"}],\"stream\":false,\"options\":{\"num_predict\":1}}" >/dev/null 2>&1 && \
  echo "Lade qwen3:32b..." && \
  curl -sf --max-time 300 http://localhost:11434/api/chat \
    -d "{\"model\":\"qwen3:32b\",\"messages\":[{\"role\":\"user\",\"content\":\"warmup\"}],\"stream\":false,\"options\":{\"num_predict\":1}}" >/dev/null 2>&1 && \
  echo "Alle Modelle geladen."'
TimeoutStartSec=600
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

# Health-Check Service (prüft ob Modelle geladen, lädt nach wenn nötig)
cat > /tmp/evo-ollama-healthcheck.service << 'EOF'
[Unit]
Description=Ollama Health-Check und Modell-Reload
After=ollama.service

[Service]
Type=oneshot
ExecStart=/bin/bash -c '\
  RUNNING=$(curl -sf http://localhost:11434/api/ps 2>/dev/null | python3 -c "import sys,json; print(len(json.load(sys.stdin).get(\"models\",[])))" 2>/dev/null || echo 0); \
  if [ "$RUNNING" -lt 2 ]; then \
    echo "Nur $RUNNING Modelle geladen — starte Preload"; \
    systemctl start ollama-preload; \
  else \
    echo "$RUNNING Modelle geladen — OK"; \
  fi'
EOF

cat > /tmp/evo-ollama-healthcheck.timer << 'EOF'
[Unit]
Description=Ollama Health-Check alle 60s

[Timer]
OnBootSec=120
OnUnitActiveSec=60
AccuracySec=10

[Install]
WantedBy=timers.target
EOF

# ---------------------------------------------------------------------------
# 2. Auf EVO-X2 deployen (braucht sudo)
# ---------------------------------------------------------------------------

echo "Kopiere Dateien auf EVO-X2 ..."
scp $SSH_OPTS \
    /tmp/evo-ollama-override.conf \
    /tmp/evo-99-recovery.conf \
    /tmp/evo-ollama-preload.service \
    /tmp/evo-ollama-healthcheck.service \
    /tmp/evo-ollama-healthcheck.timer \
    "$EVO_HOST:/tmp/"

echo ""
echo "Installiere auf EVO-X2 (sudo noetig) ..."
ssh -t $SSH_OPTS "$EVO_HOST" '
set -e

# Ollama Override
sudo mv /tmp/evo-ollama-override.conf /etc/systemd/system/ollama.service.d/override.conf

# Kernel Panic + OOM
sudo mv /tmp/evo-99-recovery.conf /etc/sysctl.d/99-evo-recovery.conf
sudo sysctl --system > /dev/null 2>&1

# Preload Service
sudo mv /tmp/evo-ollama-preload.service /etc/systemd/system/ollama-preload.service

# Health-Check Timer
sudo mv /tmp/evo-ollama-healthcheck.service /etc/systemd/system/ollama-healthcheck.service
sudo mv /tmp/evo-ollama-healthcheck.timer /etc/systemd/system/ollama-healthcheck.timer

# Aktivieren
sudo systemctl daemon-reload
sudo systemctl enable ollama-preload
sudo systemctl enable --now ollama-healthcheck.timer
sudo systemctl restart ollama

echo ""
echo "=========================================="
echo "  Verifizierung"
echo "=========================================="
echo ""
echo "--- Ollama Override ---"
cat /etc/systemd/system/ollama.service.d/override.conf
echo ""
echo "--- Kernel Panic ---"
sysctl kernel.panic kernel.panic_on_oops vm.panic_on_oom
echo ""
echo "--- Services ---"
systemctl is-enabled ollama ollama-preload ollama-healthcheck.timer
echo ""
echo "--- Timer aktiv ---"
systemctl list-timers ollama-healthcheck.timer --no-pager
'

echo ""
echo "============================================"
echo "  Fix installiert."
echo ""
echo "  WICHTIG: Im BIOS pruefen:"
echo "  Advanced → Power On After Power Fail → [Enabled]"
echo "  (Sonst startet EVO-X2 nach Shelly-Cycle nicht!)"
echo "============================================"
