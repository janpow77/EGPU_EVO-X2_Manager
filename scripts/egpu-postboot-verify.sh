#!/usr/bin/env bash
set -uo pipefail

LOG_DIR="/var/lib/egpu-manager"
LOG_FILE="$LOG_DIR/egpu-postboot-verify.log"
EGPU_PCI="${EGPU_PCI:-0000:05:00.0}"
ROOT_PORT="${ROOT_PORT:-0000:00:07.0}"

mkdir -p "$LOG_DIR"

{
  echo "=== eGPU Post-Boot Verification ==="
  date -Is
  echo

  echo "--- Kernel cmdline ---"
  cat /proc/cmdline || true
  echo

  echo "--- systemd ---"
  systemctl is-active egpu-gpu-init.service egpu-managerd.service bolt.service 2>/dev/null || true
  systemctl status egpu-gpu-init.service egpu-managerd.service --no-pager 2>/dev/null || true
  echo

  echo "--- Thunderbolt ---"
  boltctl list 2>/dev/null || true
  if [ -e /sys/bus/thunderbolt/devices/0-3/authorized ]; then
    echo "authorized=$(cat /sys/bus/thunderbolt/devices/0-3/authorized 2>/dev/null || true)"
    echo "runtime_pm=$(cat /sys/bus/thunderbolt/devices/0-3/power/control 2>/dev/null || true)"
  fi
  echo

  echo "--- PCI tree ---"
  lspci -tv 2>/dev/null || true
  echo

  echo "--- Thunderbolt root port $ROOT_PORT ---"
  lspci -vv -s "$ROOT_PORT" 2>/dev/null || true
  echo "bus_range_register=$(setpci -s "$ROOT_PORT" 18.l 2>/dev/null || true)"
  echo

  echo "--- eGPU PCI $EGPU_PCI ---"
  lspci -nnk -s "$EGPU_PCI" 2>/dev/null || true
  if [ -d "/sys/bus/pci/devices/$EGPU_PCI" ]; then
    echo "vendor=$(cat "/sys/bus/pci/devices/$EGPU_PCI/vendor" 2>/dev/null || true)"
    echo "power_control=$(cat "/sys/bus/pci/devices/$EGPU_PCI/power/control" 2>/dev/null || true)"
    echo "link_speed=$(cat "/sys/bus/pci/devices/$EGPU_PCI/current_link_speed" 2>/dev/null || true)"
    echo "link_width=$(cat "/sys/bus/pci/devices/$EGPU_PCI/current_link_width" 2>/dev/null || true)"
    echo "driver=$(basename "$(readlink "/sys/bus/pci/devices/$EGPU_PCI/driver" 2>/dev/null)" 2>/dev/null || true)"
  else
    echo "PCI device missing"
  fi
  echo

  echo "--- NVIDIA ---"
  nvidia-smi --query-gpu=pci.bus_id,name,memory.total,driver_version,pstate,power.draw --format=csv,noheader 2>/dev/null || true
  echo

  echo "--- Result ---"
  cmdline_ok=false
  pci_ok=false
  nvml_ok=false

  grep -q 'hpbridge=16' /proc/cmdline && cmdline_ok=true
  lspci -s "$EGPU_PCI" >/dev/null 2>&1 && pci_ok=true
  nvidia-smi --query-gpu=pci.bus_id,name --format=csv,noheader 2>/dev/null | grep -q '05:00.0' && nvml_ok=true

  echo "cmdline_hpbridge=$cmdline_ok"
  echo "egpu_pci_present=$pci_ok"
  echo "egpu_nvml_visible=$nvml_ok"

  if [ "$cmdline_ok" = true ] && [ "$pci_ok" = true ] && [ "$nvml_ok" = true ]; then
    echo "overall=pass"
  elif [ "$cmdline_ok" = true ] && [ "$pci_ok" = true ]; then
    echo "overall=partial-pass-pci-present"
  else
    echo "overall=fail"
  fi
} >"$LOG_FILE" 2>&1

systemctl disable egpu-postboot-verify.service >/dev/null 2>&1 || true
exit 0
