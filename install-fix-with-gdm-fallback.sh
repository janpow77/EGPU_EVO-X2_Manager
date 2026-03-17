#!/bin/bash
set -e

echo "=== eGPU Manager Fix Installation (mit GDM-Fallback) ==="
echo ""
echo "Änderungen:"
echo "  - TDP-Limit: 250W → 300W (mehr Headroom für RTX 5070 Ti Boost)"
echo "  - Health-Score Critical: 40 → 20 (weniger pessimistisch)"
echo "  - Health-Score Warning: 60 → 40"
echo "  - Recovery-Rate: 1.0 → 3.0 (schnellere Erholung)"
echo "  - Penalties reduziert (AER: 3→2, PCIe: 5→3, Thermal: 5→2)"
echo "  - GDM-Fallback: Erzwinge Intel iGPU für Login-Screen"
echo ""

# Backup der aktuellen Konfiguration
echo "[1/7] Erstelle Backup der config.toml..."
cp /etc/egpu-manager/config.toml /etc/egpu-manager/config.toml.backup.$(date +%s)

# Aktualisiere config.toml mit gelockerten Health-Score-Parametern
echo "[2/7] Aktualisiere Health-Score-Parameter in config.toml..."
sed -i 's/^health_score_aer_penalty = 3.0/health_score_aer_penalty = 2.0/' /etc/egpu-manager/config.toml
sed -i 's/^health_score_pcie_error_penalty = 5.0/health_score_pcie_error_penalty = 3.0/' /etc/egpu-manager/config.toml
sed -i 's/^health_score_smi_slow_penalty = 2.0/health_score_smi_slow_penalty = 1.0/' /etc/egpu-manager/config.toml
sed -i 's/^health_score_thermal_penalty = 5.0/health_score_thermal_penalty = 2.0/' /etc/egpu-manager/config.toml
sed -i 's/^health_score_recovery_per_minute = 1.0/health_score_recovery_per_minute = 3.0/' /etc/egpu-manager/config.toml
sed -i 's/^health_score_warning_threshold = 60.0/health_score_warning_threshold = 40.0/' /etc/egpu-manager/config.toml
sed -i 's/^health_score_critical_threshold = 40.0/health_score_critical_threshold = 20.0/' /etc/egpu-manager/config.toml

# Stoppe den laufenden Service vor dem Ersetzen des Binaries
echo "[3/7] Stoppe egpu-managerd Service..."
systemctl stop egpu-managerd.service || true

# Installiere neues Binary (mit TDP-Fix im Code)
echo "[4/7] Installiere neues egpu-managerd Binary..."
cp /home/janpow/Projekte/egpu/target/release/egpu-managerd /usr/local/bin/egpu-managerd
chmod +x /usr/local/bin/egpu-managerd

# ZUSÄTZLICH: GDM-Fallback-Konfiguration für Intel iGPU
echo "[5/7] Erstelle GDM-Fallback-Konfiguration (Intel iGPU)..."
cat > /etc/X11/xorg.conf.d/00-gdm-intel-fallback.conf << 'XORG_EOF'
# GDM Fallback: Nutze Intel iGPU für Login-Screen
# Verhindert Crashes bei eGPU-Problemen
Section "ServerLayout"
    Identifier "GDM-Layout"
    Screen 0 "Intel-Screen" 0 0
EndSection

Section "Device"
    Identifier "Intel-GPU"
    Driver "modesetting"
    BusID "PCI:0:2:0"
    Option "AccelMethod" "glamor"
EndSection

Section "Screen"
    Identifier "Intel-Screen"
    Device "Intel-GPU"
EndSection
XORG_EOF

# Starte egpu-managerd wieder
echo "[6/7] Starte egpu-managerd Service..."
systemctl start egpu-managerd.service

# Verifiziere
echo "[7/7] Verifiziere..."
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
echo "  ✓ GDM-Fallback auf Intel iGPU konfiguriert"
echo ""
echo "HINWEIS: GDM wird NICHT neugestartet (das kann Display-Freeze verursachen)."
echo "Die GDM-Fallback-Config wird beim naechsten Login/Reboot aktiv."
echo ""
echo "Backup: /etc/egpu-manager/config.toml.backup.*"
echo "GDM-Fallback: /etc/X11/xorg.conf.d/00-gdm-intel-fallback.conf"
