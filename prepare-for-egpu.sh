#!/bin/bash
# eGPU Preparation Script (Pre-Reboot)
# Dieses Skript bereitet das System auf den eGPU-Betrieb nach dem nächsten Start vor.
# 🛡️ SICHERHEITS-GARANTIE:
# - KEINE Installation von Grafiktreibern.
# - KEIN Neustart der grafischen Oberfläche (GUI).
# - NUR Konfigurations-Updates für den nächsten Boot.

set -euo pipefail

echo "=== eGPU Vorbereitung (NVIDIA Blackwell & TB5) ==="

if [ "$(id -u)" -ne 0 ]; then
    echo "FEHLER: Bitte mit sudo ausführen: sudo bash $0"
    exit 1
fi

# 1. Manager-Software und Berechtigungen aktualisieren
echo "[1/3] Aktualisiere eGPU-Manager Software..."
# Dies nutzt das existierende deploy.sh, das nur Dateien kopiert
bash deploy.sh

# 2. Boot-Konfiguration für Blackwell (RTX 5070) vorbereiten
echo "[2/3] Bereite Boot-Parameter vor (hpbridge=16 für TB5)..."
# Dies nutzt fix-boot-order.sh für GRUB-Einträge und Boot-Reihenfolge
bash fix-boot-order.sh

echo ""
echo "=== ✅ VORBEREITUNG ABGESCHLOSSEN ==="
echo "Dein System ist jetzt bereit für den eGPU-Betrieb."
echo ""
echo "WICHTIGE HINWEISE FÜR SPÄTER:"
echo "1. Beende deine Arbeit in Ruhe."
echo "2. Führe dann einen echten POWER-OFF durch (kein Reboot):"
echo "   Befehl: sudo poweroff"
echo "3. Warte 15 Sekunden, damit die Kondensatoren der RTX 5070 entladen."
echo "4. Schalte den NUC wieder ein."
echo ""
echo "INFO: Es wurde KEIN Grafiktreiber geändert. Version 590.48.01 bleibt aktiv."
