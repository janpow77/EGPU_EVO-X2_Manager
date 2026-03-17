#!/bin/bash
set -e

echo "=== eGPU Manager Fix Installation ==="
echo ""
echo "Änderungen:"
echo "  - TDP-Limit: 250W → 300W (mehr Headroom für RTX 5070 Ti Boost)"
echo "  - Health-Score Critical: 40 → 20 (weniger pessimistisch)"
echo "  - Health-Score Warning: 60 → 40"
echo "  - Recovery-Rate: 1.0 → 3.0 (schnellere Erholung)"
echo "  - Penalties reduziert (AER: 3→2, PCIe: 5→3, Thermal: 5→2)"
echo ""

# Backup der aktuellen Konfiguration
echo "[1/5] Erstelle Backup der config.toml..."
cp /etc/egpu-manager/config.toml /etc/egpu-manager/config.toml.backup.$(date +%s)

# Aktualisiere config.toml mit gelockerten Health-Score-Parametern
echo "[2/5] Aktualisiere Health-Score-Parameter in config.toml..."
sed -i 's/^health_score_aer_penalty = 3.0/health_score_aer_penalty = 2.0/' /etc/egpu-manager/config.toml
sed -i 's/^health_score_pcie_error_penalty = 5.0/health_score_pcie_error_penalty = 3.0/' /etc/egpu-manager/config.toml
sed -i 's/^health_score_smi_slow_penalty = 2.0/health_score_smi_slow_penalty = 1.0/' /etc/egpu-manager/config.toml
sed -i 's/^health_score_thermal_penalty = 5.0/health_score_thermal_penalty = 2.0/' /etc/egpu-manager/config.toml
sed -i 's/^health_score_recovery_per_minute = 1.0/health_score_recovery_per_minute = 3.0/' /etc/egpu-manager/config.toml
sed -i 's/^health_score_warning_threshold = 60.0/health_score_warning_threshold = 40.0/' /etc/egpu-manager/config.toml
sed -i 's/^health_score_critical_threshold = 40.0/health_score_critical_threshold = 20.0/' /etc/egpu-manager/config.toml

# Installiere neues Binary (mit TDP-Fix im Code)
echo "[3/5] Installiere neues egpu-managerd Binary..."
cp /home/janpow/Projekte/egpu/target/release/egpu-managerd /usr/local/bin/egpu-managerd
chmod +x /usr/local/bin/egpu-managerd

# Stoppe egpu-managerd vor Binary-Ersetzung
echo "[4/6] Stoppe egpu-managerd Service..."
systemctl stop egpu-managerd.service || true
sleep 1

# Starte egpu-managerd neu
echo "[5/6] Starte egpu-managerd Service..."
systemctl start egpu-managerd.service

# Verifiziere Daemon-Start
echo "[6/6] Verifiziere Daemon-Start..."
sleep 2
if systemctl is-active --quiet egpu-managerd.service; then
    echo "  egpu-managerd laeuft."
else
    echo "  WARNUNG: egpu-managerd konnte nicht gestartet werden (eGPU abgesteckt?)."
    echo "  Service startet automatisch wenn eGPU verfuegbar ist."
fi

echo ""
echo "✓ Installation erfolgreich abgeschlossen!"
echo ""
echo "Die folgenden Fixes wurden angewendet:"
echo "  ✓ TDP-Limit erhöht (250W → 300W)"
echo "  ✓ Health-Score-Schwellwerte angepasst"
echo "  ✓ egpu-managerd neu installiert"
echo ""
echo "HINWEIS: GDM wird NICHT neugestartet (das kann Display-Freeze verursachen)."
echo "Falls GDM Probleme macht, reboot stattdessen."
echo ""
echo "Backup der alten config.toml: /etc/egpu-manager/config.toml.backup.*"
