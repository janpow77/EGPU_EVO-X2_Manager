#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [ "$(id -u)" -ne 0 ]; then
  echo "Dieses Skript muss als root laufen: sudo bash scripts/apply-egpu-tb4-hotplug-fix.sh" >&2
  exit 1
fi

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Fehlt: $1" >&2
    exit 1
  fi
}

backup_file() {
  local path="$1"
  if [ -e "$path" ]; then
    cp -a "$path" "$path.bak.$(date +%Y%m%d%H%M%S)"
  fi
}

set_kernel_cmdline() {
  local grub_file="/etc/default/grub"
  local wanted=(
    "iommu=pt"
    "pcie_aspm=off"
    "pcie_port_pm=off"
    "pci=realloc,assign-busses,hpbridge=16"
  )

  backup_file "$grub_file"

  if ! grep -q '^GRUB_CMDLINE_LINUX_DEFAULT=' "$grub_file"; then
    echo 'GRUB_CMDLINE_LINUX_DEFAULT=""' >>"$grub_file"
  fi

  local line value token new_value
  line="$(grep '^GRUB_CMDLINE_LINUX_DEFAULT=' "$grub_file" | tail -1)"
  value="${line#*=}"
  value="${value#\"}"
  value="${value%\"}"

  local kept=()
  for token in $value; do
    case "$token" in
      iommu=*|pcie_aspm=*|pcie_port_pm=*|pci=*)
        ;;
      *)
        kept+=("$token")
        ;;
    esac
  done

  kept+=("${wanted[@]}")
  new_value="${kept[*]}"
  sed -i "s|^GRUB_CMDLINE_LINUX_DEFAULT=.*|GRUB_CMDLINE_LINUX_DEFAULT=\"${new_value}\"|" "$grub_file"
}

write_gpu_init_unit() {
  cat >/etc/systemd/system/egpu-gpu-init.service <<'UNIT'
[Unit]
Description=eGPU GPU Init (Thunderbolt Link Training + nvidia modprobe)
After=bolt.service
Wants=bolt.service
Before=nvidia-persistenced.service egpu-managerd.service
ConditionPathExists=/sys/bus/thunderbolt

[Service]
Type=oneshot
RemainAfterExit=no
ExecStart=/usr/local/bin/egpu-gpu-init.sh
TimeoutStartSec=120

[Install]
WantedBy=multi-user.target
UNIT
}

write_udev_rule() {
  cat >/etc/udev/rules.d/99-egpu-thunderbolt.rules <<'UDEV'
# Wenn das Razer Core X V2 erscheint, PCIe-Tunnel rescannen und nvidia probe starten.
# Wichtig: Die PCI-Funktion 0000:05:00.0 existiert in Fehlerfaellen noch nicht.
ACTION=="add", SUBSYSTEM=="thunderbolt", KERNEL=="0-3", ATTR{unique_id}=="8ab48780-00c3-eba8-ffff-ffffffffffff", \
    TAG+="systemd", ENV{SYSTEMD_WANTS}+="egpu-gpu-init.service"
ACTION=="change", SUBSYSTEM=="thunderbolt", KERNEL=="0-3", ATTR{unique_id}=="8ab48780-00c3-eba8-ffff-ffffffffffff", \
    TAG+="systemd", ENV{SYSTEMD_WANTS}+="egpu-gpu-init.service"

# Fallback: Wenn die GPU-Funktion bereits auf dem PCI-Bus erscheint, ebenfalls initialisieren.
ACTION=="add", SUBSYSTEM=="pci", KERNEL=="0000:05:00.0", ATTR{vendor}=="0x10de", \
    TAG+="systemd", ENV{SYSTEMD_WANTS}+="egpu-gpu-init.service"
UDEV
}

write_postboot_verify_unit() {
  cat >/etc/systemd/system/egpu-postboot-verify.service <<'UNIT'
[Unit]
Description=eGPU Post-Boot Verification
After=multi-user.target bolt.service egpu-gpu-init.service egpu-managerd.service
Wants=bolt.service

[Service]
Type=oneshot
ExecStart=/usr/local/bin/egpu-postboot-verify.sh
TimeoutStartSec=90

[Install]
WantedBy=multi-user.target
UNIT
}

require_cmd timeshift
require_cmd update-grub
require_cmd systemctl
require_cmd udevadm

if [ ! -f scripts/egpu-gpu-init.sh ]; then
  echo "Fehlt: scripts/egpu-gpu-init.sh" >&2
  exit 1
fi
if [ ! -f scripts/egpu-postboot-verify.sh ]; then
  echo "Fehlt: scripts/egpu-postboot-verify.sh" >&2
  exit 1
fi
if [ ! -f egpu-managerd.service ]; then
  echo "Fehlt: egpu-managerd.service" >&2
  exit 1
fi

echo "=== eGPU TB4 Hotplug Fix anwenden ==="
echo "[1/8] Timeshift-Snapshot erstellen"
timeshift --create --comments "pre-egpu-tb4-hotplug-fix $(date -Is)" --tags D

echo "[2/8] Dateien sichern"
backup_file /etc/systemd/system/egpu-gpu-init.service
backup_file /etc/systemd/system/egpu-managerd.service
backup_file /etc/systemd/system/egpu-postboot-verify.service
backup_file /etc/udev/rules.d/99-egpu-thunderbolt.rules
backup_file /usr/local/bin/egpu-gpu-init.sh
backup_file /usr/local/bin/egpu-postboot-verify.sh

echo "[3/8] Skripte installieren"
mkdir -p /var/lib/egpu-manager
chown -R root:root /var/lib/egpu-manager
install -m 755 scripts/egpu-gpu-init.sh /usr/local/bin/egpu-gpu-init.sh
install -m 755 scripts/egpu-postboot-verify.sh /usr/local/bin/egpu-postboot-verify.sh

echo "[4/8] systemd-Units installieren"
write_gpu_init_unit
install -m 644 egpu-managerd.service /etc/systemd/system/egpu-managerd.service
write_postboot_verify_unit

echo "[5/8] udev-Regel installieren"
write_udev_rule

echo "[6/8] GRUB-Kernelparameter vorbereiten"
set_kernel_cmdline
update-grub

echo "[7/8] systemd/udev neu laden"
systemctl daemon-reload
udevadm control --reload-rules
systemctl enable egpu-gpu-init.service
systemctl enable egpu-managerd.service
systemctl enable egpu-postboot-verify.service
systemctl stop egpu-gpu-init.service 2>/dev/null || true

echo "[8/8] Installierten Zustand pruefen"
systemctl cat egpu-gpu-init.service >/dev/null
systemctl cat egpu-managerd.service >/dev/null
test -x /usr/local/bin/egpu-gpu-init.sh
test -x /usr/local/bin/egpu-postboot-verify.sh
grep -q 'hpbridge=16' /etc/default/grub
grep -q 'SUBSYSTEM=="thunderbolt"' /etc/udev/rules.d/99-egpu-thunderbolt.rules

echo
echo "=== Fertig ==="
echo "Naechster Schritt: echter Kaltstart, kein Reboot."
echo "1. sudo poweroff"
echo "2. eGPU 10-15 Sekunden stromlos machen"
echo "3. eGPU einschalten"
echo "4. NUC einschalten"
echo
echo "Nach dem Boot schreibt egpu-postboot-verify.service automatisch:"
echo "  /var/lib/egpu-manager/egpu-postboot-verify.log"
