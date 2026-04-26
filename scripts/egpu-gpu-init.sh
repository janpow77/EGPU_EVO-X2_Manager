#!/usr/bin/env bash
set -euo pipefail

EGPU_PCI="${EGPU_PCI:-0000:05:00.0}"
EGPU_AUDIO="${EGPU_AUDIO:-0000:05:00.1}"
ROOT_PORT="${ROOT_PORT:-0000:00:07.0}"
TB_DEVICE="${TB_DEVICE:-0-3}"
MAX_WAIT="${MAX_WAIT:-60}"
AUTH_SETTLE_SECS="${AUTH_SETTLE_SECS:-8}"
POST_RESCAN_SETTLE_SECS="${POST_RESCAN_SETTLE_SECS:-8}"
CMPLTO_VALUE="${CMPLTO_VALUE:-0xA}"
REQUIRE_EGPU=false
DRIVERS_AUTOPROBE_PATH="/sys/bus/pci/drivers_autoprobe"
ORIGINAL_DRIVERS_AUTOPROBE=""
UPSTREAMS=("0000:04:00.0" "0000:03:00.0" "$ROOT_PORT")

for arg in "$@"; do
  case "$arg" in
    --require-egpu)
      REQUIRE_EGPU=true
      ;;
    --help)
      cat <<'EOF'
Verwendung: sudo bash egpu-gpu-init.sh [--require-egpu]

  --require-egpu   Mit Exit-Code 1 beenden wenn die eGPU am Ende nicht in NVML sichtbar ist.
EOF
      exit 0
      ;;
    *)
      echo "Unbekanntes Argument: $arg" >&2
      exit 2
      ;;
  esac
done

log() {
  echo "[egpu-init] $*"
}

warn() {
  echo "[egpu-init][warn] $*" >&2
}

require_root() {
  if [ "$(id -u)" -ne 0 ]; then
    echo "Dieses Skript muss als root laufen." >&2
    exit 1
  fi
}

restore_drivers_autoprobe() {
  if [ -n "${ORIGINAL_DRIVERS_AUTOPROBE:-}" ] && [ -e "$DRIVERS_AUTOPROBE_PATH" ]; then
    echo "$ORIGINAL_DRIVERS_AUTOPROBE" >"$DRIVERS_AUTOPROBE_PATH" 2>/dev/null || true
  fi
}

trap restore_drivers_autoprobe EXIT

device_path() {
  printf '/sys/bus/pci/devices/%s' "$1"
}

device_exists() {
  [ -d "$(device_path "$1")" ]
}

device_vendor() {
  local dev="$1"
  if device_exists "$dev"; then
    cat "$(device_path "$dev")/vendor" 2>/dev/null || echo "0xffff"
  else
    echo "0xffff"
  fi
}

device_responsive() {
  local vendor
  vendor="$(device_vendor "$1")"
  [ "$vendor" != "0xffff" ] && [ "$vendor" != "0xFFFF" ] && [ -n "$vendor" ]
}

read_link_driver() {
  local link
  link="$(readlink "$(device_path "$1")/driver" 2>/dev/null || true)"
  if [ -n "$link" ]; then
    basename "$link"
  else
    echo "none"
  fi
}

set_power_on() {
  local dev="$1"
  local power_path
  power_path="$(device_path "$dev")/power/control"
  if [ -e "$power_path" ]; then
    echo on >"$power_path" 2>/dev/null || true
  fi
}

set_thunderbolt_power_on() {
  local power_path
  for power_path in /sys/bus/thunderbolt/devices/*/power/control; do
    [ -e "$power_path" ] || continue
    echo on >"$power_path" 2>/dev/null || true
  done
}

set_driver_override() {
  local dev="$1"
  local value="$2"
  local override_path
  override_path="$(device_path "$dev")/driver_override"
  if [ -e "$override_path" ]; then
    printf '%s' "$value" >"$override_path" 2>/dev/null || true
  fi
}

block_audio_autobind() {
  if ! device_exists "$EGPU_AUDIO"; then
    return 0
  fi

  set_driver_override "$EGPU_AUDIO" "none"

  if [ -e "$(device_path "$EGPU_AUDIO")/driver/unbind" ]; then
    echo "$EGPU_AUDIO" >"$(device_path "$EGPU_AUDIO")/driver/unbind" 2>/dev/null || true
  fi
}

set_drivers_autoprobe() {
  local value="$1"
  if [ ! -e "$DRIVERS_AUTOPROBE_PATH" ]; then
    return 0
  fi

  if [ -z "$ORIGINAL_DRIVERS_AUTOPROBE" ]; then
    ORIGINAL_DRIVERS_AUTOPROBE="$(cat "$DRIVERS_AUTOPROBE_PATH" 2>/dev/null || echo "1")"
  fi

  echo "$value" >"$DRIVERS_AUTOPROBE_PATH"
}

nvidia_has_egpu() {
  nvidia-smi --query-gpu=pci.bus_id,name --format=csv,noheader 2>/dev/null | grep -q "05:00.0"
}

wait_for_tb_authorized() {
  local auth_path="/sys/bus/thunderbolt/devices/$TB_DEVICE/authorized"
  local waited=0

  set_thunderbolt_power_on

  if [ ! -e "$auth_path" ]; then
    warn "Thunderbolt-Auth-Pfad fehlt: $auth_path"
    return 1
  fi

  while [ "$waited" -lt "$MAX_WAIT" ]; do
    if [ "$(cat "$auth_path" 2>/dev/null || echo "0")" = "1" ]; then
      log "Thunderbolt authorized nach ${waited}s"
      return 0
    fi
    sleep 1
    waited=$((waited + 1))
  done

  warn "Thunderbolt-Authorization Timeout nach ${MAX_WAIT}s"
  return 1
}

tune_root_port_completion_timeout() {
  if ! command -v setpci >/dev/null 2>&1; then
    return 0
  fi

  if ! device_exists "$ROOT_PORT"; then
    return 0
  fi

  local current
  current="$(setpci -s "$ROOT_PORT" 0xd4.w 2>/dev/null || true)"
  if [ -z "$current" ]; then
    return 0
  fi

  if [ "$current" != "$CMPLTO_VALUE" ]; then
    if setpci -s "$ROOT_PORT" "0xd4.w=$CMPLTO_VALUE" 2>/dev/null; then
      log "Root-Port $ROOT_PORT Completion Timeout: $current -> $CMPLTO_VALUE"
    else
      warn "Completion-Timeout-Setzen fehlgeschlagen auf $ROOT_PORT"
    fi
  fi
}

remove_device() {
  local dev="$1"
  if ! device_exists "$dev"; then
    return 0
  fi

  if [ -e "$(device_path "$dev")/driver/unbind" ]; then
    echo "$dev" >"$(device_path "$dev")/driver/unbind" 2>/dev/null || true
  fi
  if [ -e "$(device_path "$dev")/remove" ]; then
    echo 1 >"$(device_path "$dev")/remove" 2>/dev/null || true
  fi
}

reset_with_setpci() {
  local bridge="$1"
  if ! command -v setpci >/dev/null 2>&1; then
    return 1
  fi

  local bridge_ctl reset_ctl
  bridge_ctl="$(setpci -s "$bridge" BRIDGE_CONTROL 2>/dev/null || true)"
  if ! [[ "$bridge_ctl" =~ ^[0-9A-Fa-f]{4}$ ]]; then
    return 1
  fi

  reset_ctl="$(printf '%04x' "$((0x$bridge_ctl | 0x0040))")"
  setpci -s "$bridge" "BRIDGE_CONTROL=$reset_ctl" 2>/dev/null || return 1
  sleep 1
  setpci -s "$bridge" "BRIDGE_CONTROL=$bridge_ctl" 2>/dev/null || return 1
  sleep 1
  return 0
}

reset_upstream_bridge() {
  local bridge="$1"
  set_power_on "$bridge"

  if [ -e "$(device_path "$bridge")/reset_subordinate" ]; then
    log "Reset via $bridge/reset_subordinate"
    echo 1 >"$(device_path "$bridge")/reset_subordinate" 2>/dev/null || true
    sleep 2
    return 0
  fi

  if [ -e "$(device_path "$bridge")/reset" ]; then
    if echo 1 >"$(device_path "$bridge")/reset" 2>/dev/null; then
      log "Reset via $bridge/reset"
      sleep 2
      return 0
    fi
  fi

  if reset_with_setpci "$bridge"; then
    log "Hot Reset via setpci auf $bridge"
    return 0
  fi

  warn "Kein verwendbarer Reset-Hook für $bridge"
  return 1
}

wait_for_device_responsive() {
  local timeout="$1"
  local waited=0
  while [ "$waited" -lt "$timeout" ]; do
    if device_responsive "$EGPU_PCI"; then
      log "PCI-Device $EGPU_PCI nach ${waited}s responsiv"
      return 0
    fi
    sleep 1
    waited=$((waited + 1))
  done

  warn "PCI-Device $EGPU_PCI nach ${timeout}s nicht responsiv"
  return 1
}

show_state() {
  echo
  echo "--- PCI ---"
  lspci -nnk -s "$EGPU_PCI" || true

  echo
  echo "--- NVIDIA ---"
  nvidia-smi --query-gpu=pci.bus_id,name --format=csv,noheader 2>/dev/null || true

  echo
  echo "--- sysfs ---"
  if device_exists "$EGPU_PCI"; then
    cat "$(device_path "$EGPU_PCI")/power_state" 2>/dev/null || true
    cat "$(device_path "$EGPU_PCI")/current_link_speed" 2>/dev/null || true
    cat "$(device_path "$EGPU_PCI")/current_link_width" 2>/dev/null || true
    cat "$(device_path "$EGPU_PCI")/enable" 2>/dev/null || true
    read_link_driver "$EGPU_PCI" || true
  else
    echo "PCI-Device fehlt"
  fi
}

controlled_rescan() {
  set_drivers_autoprobe 0
  set_driver_override "$EGPU_PCI" "none"
  block_audio_autobind
  set_thunderbolt_power_on

  for upstream in "${UPSTREAMS[@]}"; do
    set_power_on "$upstream"
  done
  set_power_on "$EGPU_PCI"

  remove_device "$EGPU_AUDIO"
  remove_device "$EGPU_PCI"
  sleep 1

  for upstream in "${UPSTREAMS[@]}"; do
    reset_upstream_bridge "$upstream" || true
  done

  echo 1 >/sys/bus/pci/rescan
  wait_for_device_responsive 20 || true
  if command -v udevadm >/dev/null 2>&1; then
    udevadm settle --timeout=10 || true
  fi
  if device_exists "$EGPU_PCI"; then
    set_driver_override "$EGPU_PCI" "none"
  fi
  if device_exists "$EGPU_AUDIO"; then
    set_driver_override "$EGPU_AUDIO" "none"
  fi
  sleep "$POST_RESCAN_SETTLE_SECS"
}

manual_probe() {
  local enabled

  if ! device_exists "$EGPU_PCI"; then
    return 1
  fi

  block_audio_autobind
  set_driver_override "$EGPU_PCI" "nvidia"
  set_power_on "$EGPU_PCI"

  if [ -e "$(device_path "$EGPU_PCI")/enable" ]; then
    enabled="$(cat "$(device_path "$EGPU_PCI")/enable" 2>/dev/null || echo "1")"
    if [ "$enabled" = "0" ]; then
      echo 1 >"$(device_path "$EGPU_PCI")/enable" 2>/dev/null || true
    fi
  fi

  if command -v setpci >/dev/null 2>&1; then
    setpci -s "$EGPU_PCI" COMMAND=0007 2>/dev/null || true
  fi

  modprobe nvidia || true
  modprobe nvidia_uvm || true
  modprobe nvidia_drm modeset=1 || true

  echo "$EGPU_PCI" >/sys/bus/pci/drivers/nvidia/bind 2>/dev/null || true
  sleep 2
  if ! nvidia_has_egpu; then
    echo "$EGPU_PCI" >/sys/bus/pci/drivers_probe 2>/dev/null || true
  fi
  sleep 3
  nvidia_has_egpu
}

thunderbolt_reauth() {
  local auth_path="/sys/bus/thunderbolt/devices/$TB_DEVICE/authorized"

  if [ ! -e "$auth_path" ]; then
    warn "Thunderbolt-Auth-Pfad fehlt: $auth_path"
    return 1
  fi

  log "Thunderbolt deauth/reauth für $TB_DEVICE"
  echo 0 >"$auth_path"
  sleep 1
  echo 1 >"$auth_path"
  wait_for_tb_authorized || true
  sleep "$AUTH_SETTLE_SECS"
}

main() {
  require_root

  log "=== eGPU GPU Init ==="
  set_thunderbolt_power_on
  wait_for_tb_authorized || true
  sleep "$AUTH_SETTLE_SECS"

  tune_root_port_completion_timeout
  controlled_rescan

  if ! manual_probe; then
    thunderbolt_reauth || true
    controlled_rescan
    manual_probe || true
  fi

  show_state

  if nvidia_has_egpu; then
    log "eGPU ist wieder in NVIDIA sichtbar"
    exit 0
  fi

  if [ "$REQUIRE_EGPU" = true ]; then
    warn "eGPU bleibt offline"
    exit 1
  fi

  warn "eGPU nicht verfuegbar; interne GPU bleibt aktiv"
  exit 0
}

main "$@"
