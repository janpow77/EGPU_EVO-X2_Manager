#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [ "$(id -u)" -eq 0 ]; then
  SUDO=""
else
  SUDO="sudo"
fi

BIN="target/release/egpu-managerd"
INIT_SCRIPT_SOURCE="scripts/egpu-gpu-init.sh"
EGPU_PCI="0000:05:00.0"

if [ ! -x "$BIN" ]; then
  echo "Fehlt: $BIN"
  echo "Bitte zuerst: cargo build -p egpu-managerd --release"
  exit 1
fi

if [ ! -f "$INIT_SCRIPT_SOURCE" ]; then
  echo "Fehlt: $INIT_SCRIPT_SOURCE"
  exit 1
fi

show_state() {
  echo ""
  echo "--- PCI ---"
  lspci -nnk -s "$EGPU_PCI" || true

  echo ""
  echo "--- NVIDIA ---"
  nvidia-smi --query-gpu=pci.bus_id,name --format=csv,noheader 2>/dev/null || true

  echo ""
  echo "--- sysfs ---"
  if [ -d "/sys/bus/pci/devices/$EGPU_PCI" ]; then
    cat "/sys/bus/pci/devices/$EGPU_PCI/power_state" 2>/dev/null || true
    cat "/sys/bus/pci/devices/$EGPU_PCI/current_link_speed" 2>/dev/null || true
    cat "/sys/bus/pci/devices/$EGPU_PCI/current_link_width" 2>/dev/null || true
    cat "/sys/bus/pci/devices/$EGPU_PCI/enable" 2>/dev/null || true
    basename "$(readlink "/sys/bus/pci/devices/$EGPU_PCI/driver" 2>/dev/null)" 2>/dev/null || true
  else
    echo "PCI-Device fehlt"
  fi
}

nvidia_has_egpu() {
  nvidia-smi --query-gpu=pci.bus_id,name --format=csv,noheader 2>/dev/null | grep -q "05:00.0"
}

stop_manager() {
  echo "[stop] egpu-managerd"
  $SUDO systemctl stop egpu-managerd || true
  if systemctl is-active --quiet egpu-managerd; then
    echo "[stop] egpu-managerd haengt, sende SIGKILL"
    $SUDO systemctl kill -s SIGKILL egpu-managerd || true
    sleep 1
  fi
}

echo "=== Deploy + No-Reboot eGPU Recovery ==="

echo "[1/4] Installiere neues egpu-managerd Binary und Init-Skript"
$SUDO install -m 755 "$BIN" /usr/local/bin/egpu-managerd
$SUDO install -m 755 "$INIT_SCRIPT_SOURCE" /usr/local/bin/egpu-gpu-init.sh

echo "[2/4] Stoppe Manager"
stop_manager

echo "[3/4] Live-Recovery ohne Reboot"
if ! $SUDO /usr/local/bin/egpu-gpu-init.sh --require-egpu; then
  echo "[recover] egpu-gpu-init meldet weiterhin Fehler"
fi

echo "[4/4] Starte Manager"
$SUDO systemctl start egpu-managerd || true
sleep 5

echo ""
echo "=== Ergebnis ==="
systemctl status egpu-managerd --no-pager || true
show_state
egpu-manager status || true

if nvidia_has_egpu; then
  echo ""
  echo "eGPU ist wieder in NVIDIA sichtbar."
  exit 0
fi

echo ""
echo "eGPU bleibt offline. Falls noch nicht gemacht: Gehaeuse komplett aus,"
echo "10 Sekunden warten, wieder einschalten und dieses Skript erneut ausfuehren."
exit 1
