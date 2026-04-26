#!/bin/bash
# NVIDIA Engineering Debug Script for Barlow Ridge (TB5) & Blackwell (RTX 5070)
# Dieses Skript versucht die PCI-Bus-Adressierung manuell zu korrigieren.

BRIDGE="03:00.0"
ROOT_PORT="00:07.0"

echo "=== NVIDIA PCI Bridge Debugger ==="
if [ "$(id -u)" -ne 0 ]; then
    echo "FEHLER: Bitte mit sudo ausführen: sudo bash $0"
    exit 1
fi

echo "[1/4] Status-Check der Bridge $BRIDGE..."
# Auslesen der Bus-Konfiguration (Register 0x18: Primary, 0x19: Secondary, 0x1a: Subordinate)
CONF=$(setpci -s $BRIDGE 18.l)
echo "Aktuelle Bus-Konfiguration (Hex): $CONF"

echo "[2/4] Analyse der Root-Port-Reservierung ($ROOT_PORT)..."
ROOT_CONF=$(setpci -s $ROOT_PORT 18.l)
echo "Root-Port Bus-Range: $ROOT_CONF"
# Erwartet: 00030400 -> Primary 00, Secondary 03, Subordinate 04
# Das Problem: Wir haben nur Bus 04 frei, aber TB5 Hubs brauchen oft 2-4 Bus-Nummern!

echo "[3/4] Manueller Override-Versuch..."
# Wir versuchen die Bridge zwingend auf Secondary=04, Subordinate=04 zu setzen
# Das ist ein "Hail Mary" Pass, um zu sehen ob das Ghost-Device (RTX 5070) erscheint.
echo "Setze $BRIDGE auf Secondary=04, Subordinate=04..."
setpci -s $BRIDGE 19.b=04
setpci -s $BRIDGE 1a.b=04

echo "[4/4] Trigger PCI Rescan..."
echo 1 > /sys/bus/pci/rescan
sleep 2

echo "=== Ergebnis ==="
lspci -nnk | grep -iA 3 "NVIDIA"
if lspci -s 05:00.0 >/dev/null 2>&1; then
    echo "ERFOLG: RTX 5070 ist auf dem Bus aufgetaucht!"
else
    echo "INFO: RTX 5070 weiterhin nicht sichtbar."
    echo "Diagnose: Der Root-Port $ROOT_PORT blockiert den Adressraum (Subordinate Bus Limit)."
    echo "LÖSUNG: BIOS 'PCIe Bus Reservation' auf 10 oder höher setzen!"
fi
