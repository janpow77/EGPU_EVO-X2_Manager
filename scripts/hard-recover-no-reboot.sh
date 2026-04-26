#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [ "$(id -u)" -ne 0 ]; then
  echo "Dieses Skript muss als root laufen." >&2
  exit 1
fi

BIN="target/release/egpu-managerd"
INIT_SCRIPT_SOURCE="scripts/egpu-gpu-init.sh"
DISPLAY_MANAGER_SERVICE="${DISPLAY_MANAGER_SERVICE:-display-manager}"
GPU_DEVICES=(
  /dev/nvidiactl
  /dev/nvidia0
  /dev/nvidia-uvm
  /dev/nvidia-modeset
)

log() {
  echo "[hard-recover] $*"
}

warn() {
  echo "[hard-recover][warn] $*" >&2
}

show_status() {
  echo
  echo "--- NVIDIA ---"
  nvidia-smi --query-gpu=pci.bus_id,name --format=csv,noheader 2>/dev/null || true
  echo
  echo "--- PCI ---"
  lspci -nnk -s 05:00.0 || true
}

stop_manager() {
  log "Stoppe egpu-managerd"
  systemctl stop egpu-managerd || true
  if systemctl is-active --quiet egpu-managerd; then
    systemctl kill -s SIGKILL egpu-managerd || true
    sleep 1
  fi
}

stop_gpu_users() {
  log "Stoppe Display Manager"
  systemctl stop "$DISPLAY_MANAGER_SERVICE" || true
  systemctl stop gdm.service 2>/dev/null || true
  systemctl stop nvidia-persistenced.service 2>/dev/null || true
  sleep 3

  log "Beende verbliebene NVIDIA-Nutzer"
  fuser -k "${GPU_DEVICES[@]}" 2>/dev/null || true
  pkill -9 -f '/usr/local/bin/python3.11' 2>/dev/null || true
  pkill -9 -f 'celery' 2>/dev/null || true
  pkill -9 -f 'uvicorn' 2>/dev/null || true
  pkill -9 -f 'chrome' 2>/dev/null || true
  sleep 3
}

unload_nvidia_modules() {
  local attempt
  for attempt in $(seq 1 10); do
    if modprobe -r nvidia_drm nvidia_modeset nvidia_uvm nvidia 2>/dev/null; then
      log "NVIDIA-Module entladen"
      return 0
    fi
    warn "modprobe -r fehlgeschlagen (Versuch $attempt/10), versuche erneut"
    fuser -k "${GPU_DEVICES[@]}" 2>/dev/null || true
    sleep 2
  done

  warn "NVIDIA-Module konnten nicht entladen werden"
  lsmod | grep -E '^nvidia|^snd_hda_intel' || true
  lsof /dev/nvidia* 2>/dev/null | sed -n '1,120p' || true
  return 1
}

restart_stack() {
  log "Starte NVIDIA-Dienste und GUI erneut"
  systemctl start nvidia-persistenced.service 2>/dev/null || true
  systemctl start "$DISPLAY_MANAGER_SERVICE" || true
  systemctl start gdm.service 2>/dev/null || true
  sleep 5
}

if [ ! -x "$BIN" ]; then
  echo "Fehlt: $BIN" >&2
  echo "Bitte zuerst: cargo build -p egpu-managerd --release" >&2
  exit 1
fi

if [ ! -f "$INIT_SCRIPT_SOURCE" ]; then
  echo "Fehlt: $INIT_SCRIPT_SOURCE" >&2
  exit 1
fi

if [ -n "${DISPLAY:-}" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; then
  warn "Dieses Skript sollte aus einer Textkonsole oder per SSH gestartet werden."
fi

echo "=== Hard eGPU Recovery Ohne NUC-Reboot ==="
echo "Dieses Skript stoppt die grafische Session und entlaedt den NVIDIA-Treiber komplett."
echo

log "Installiere aktuelles egpu-managerd Binary und Init-Skript"
install -m 755 "$BIN" /usr/local/bin/egpu-managerd
install -m 755 "$INIT_SCRIPT_SOURCE" /usr/local/bin/egpu-gpu-init.sh

stop_manager
stop_gpu_users
unload_nvidia_modules

echo
echo "Jetzt die eGPU komplett AUS schalten, 10 Sekunden warten,"
echo "wieder EINSCHALTEN und dann Enter druecken."
read -r

if ! /usr/local/bin/egpu-gpu-init.sh --require-egpu; then
  warn "egpu-gpu-init meldet weiterhin Fehler"
fi

restart_stack

log "Starte egpu-managerd"
systemctl start egpu-managerd || true
sleep 5

echo
echo "=== Ergebnis ==="
systemctl status egpu-managerd --no-pager || true
show_status
egpu-manager status || true

if nvidia-smi --query-gpu=pci.bus_id,name --format=csv,noheader 2>/dev/null | grep -q '05:00.0'; then
  log "eGPU ist wieder in NVIDIA sichtbar"
  exit 0
fi

warn "eGPU bleibt offline. Der naechste Schritt waere ein erneuter Treiber-Reload in multi-user.target oder ein NUC-Reboot."
exit 1
