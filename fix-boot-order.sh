#!/bin/bash
# =============================================================================
# Fix: eGPU Boot-Probleme — Thunderbolt Timing + nvidia Module-Loading
#
# ROOT CAUSE (analysiert 2026-03-26):
#   BIOS enumeriert eGPU auf PCI-Bus BEVOR Thunderbolt-Link trainiert ist.
#   nvidia-Treiber probt sofort → Config Space = 0xFF → "fallen off the bus".
#   Das PCI-Device bleibt als "Ghost" im Kernel (vendor=0xFFFF).
#   Einfaches nvidia/bind reicht NICHT — PCI-Device muss entfernt + Bus rescannt werden.
#
# Früherer Fehler:
#   pci=noacs war gesetzt → "PCI: Unknown option 'noacs'" (existiert nicht im stock-Kernel)
#   pcie_acs_override braucht gepatchten Kernel, ist auf stock Ubuntu wirkungslos
#
# Ansatz:
#   - nvidia-Modul beim Boot BLACKLISTEN (nicht zu früh laden!)
#   - bolt.service autorisiert Thunderbolt zuerst
#   - egpu-gpu-init.service wartet auf TB-Link-Training, macht PCI-Rescan, lädt nvidia
#   - DANN startet egpu-managerd
# =============================================================================
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log()  { echo -e "${GREEN}[FIX]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
err()  { echo -e "${RED}[ERR]${NC} $1"; }

if [ "$(id -u)" -ne 0 ]; then
    err "Dieses Script muss als root laufen: sudo bash fix-boot-order.sh"
    exit 1
fi

echo "============================================="
echo " eGPU Boot-Fix v2 (Ghost-Device + TB Timing)"
echo "============================================="
echo ""

# ─── 1. GRUB: Korrekte Kernel-Parameter ─────────────────────────────────────
log "1/6: GRUB Kernel-Parameter (stock-kompatibel)..."

GRUB_FILE="/etc/default/grub"
cp "$GRUB_FILE" "${GRUB_FILE}.bak.$(date +%Y%m%d%H%M%S)"

# Nur Parameter die auf stock Ubuntu 6.8 funktionieren:
#   iommu=pt          — IOMMU passthrough (Standard für GPU-Workstations)
#   pcie_aspm=off     — PCIe Active State PM aus (stabilisiert Thunderbolt-Link)
#   pcie_port_pm=off  — PCIe Port Power-Management aus
#   pci=realloc,assign-busses — PCIe BAR-Zuweisung für Hot-Plug
#
# NICHT enthalten:
#   pci=noacs         — UNGÜLTIG auf stock-Kernel ("Unknown option")
#   pcie_acs_override — braucht gepatchten Kernel
NEW_PARAMS="iommu=pt pcie_aspm=off pcie_port_pm=off pci=realloc,assign-busses"

sed -i "s|^GRUB_CMDLINE_LINUX_DEFAULT=.*|GRUB_CMDLINE_LINUX_DEFAULT=\"${NEW_PARAMS}\"|" "$GRUB_FILE"
log "  Parameter: ${NEW_PARAMS}"
update-grub 2>&1 | tail -2
log "  GRUB aktualisiert"

# ─── 2. nvidia Module Blacklist (Boot-Zeit) ──────────────────────────────────
log "2/6: nvidia-Module beim Boot blacklisten..."

cat > /etc/modprobe.d/egpu-nvidia-defer.conf << 'MODPROBE'
# eGPU Boot-Fix: nvidia-Module nicht automatisch laden.
# Werden erst von egpu-gpu-init.service geladen nachdem
# Thunderbolt-Link trainiert ist.
# Ohne diesen Fix: NVRM "fallen off the bus" + Ghost-Device (0xFFFF)
blacklist nvidia
blacklist nvidia_drm
blacklist nvidia_uvm
blacklist nvidia_modeset
# nouveau ebenfalls verhindern
blacklist nouveau
MODPROBE

log "  /etc/modprobe.d/egpu-nvidia-defer.conf erstellt"

# ─── 3. egpu-gpu-init.service — Thunderbolt warten + nvidia laden ────────────
log "3/6: egpu-gpu-init.service (TB-Link warten → nvidia laden)..."

EGPU_PCI="0000:05:00.0"

# --- Externes Init-Script (kein Escaping-Problem in systemd) ---
cat > /usr/local/bin/egpu-gpu-init.sh << 'SCRIPT'
#!/bin/bash
# eGPU GPU Init: Thunderbolt-Authorization abwarten, dann nvidia laden.
set -euo pipefail

EGPU_PCI="0000:05:00.0"
TB_DEVICE="0-3"
MAX_WAIT=60

echo "=== eGPU GPU Init ==="
echo "Warte auf Thunderbolt-Link fuer eGPU ($EGPU_PCI)..."

waited=0
while [ $waited -lt $MAX_WAIT ]; do
    if [ -d "/sys/bus/pci/devices/$EGPU_PCI" ]; then
        VENDOR=$(cat /sys/bus/pci/devices/$EGPU_PCI/vendor 2>/dev/null || echo "0xffff")
        if [ "$VENDOR" != "0xffff" ]; then
            echo "PCI-Device erreichbar (vendor=$VENDOR) nach ${waited}s"

            # KRITISCH: Auf Thunderbolt-Authorization warten!
            # bolt.service autorisiert async — vendor=0x10de heisst NICHT
            # dass der Datenpfad steht. Ohne diese Pruefung: nvidia probe failed.
            echo "Warte auf Thunderbolt-Authorization..."
            tb_wait=0
            while [ $tb_wait -lt 30 ]; do
                TB_AUTH=$(cat /sys/bus/thunderbolt/devices/$TB_DEVICE/authorized 2>/dev/null || echo "0")
                if [ "$TB_AUTH" = "1" ]; then
                    echo "Thunderbolt authorized nach ${tb_wait}s"
                    break
                fi
                sleep 1
                tb_wait=$((tb_wait + 1))
            done

            if [ "$TB_AUTH" != "1" ]; then
                echo "WARNUNG: Thunderbolt-Authorization Timeout (30s) — versuche trotzdem"
            fi

            # Post-Authorization Settle: PCIe-Link muss vollstaendig trainiert sein
            echo "Post-Authorization Settle (3s)..."
            sleep 3
            break
        fi

        echo "Ghost-Device (vendor=0xffff) — entferne und rescanne..."
        echo 1 > /sys/bus/pci/devices/$EGPU_PCI/remove 2>/dev/null || true
        sleep 2
        echo 1 > /sys/bus/pci/rescan
        sleep 3
        waited=$((waited + 5))
        continue
    fi
    sleep 2
    waited=$((waited + 2))
done

if [ ! -d "/sys/bus/pci/devices/$EGPU_PCI" ]; then
    echo "PCI-Rescan..."
    echo 1 > /sys/bus/pci/rescan
    sleep 5
fi

echo "Lade nvidia-Module..."
modprobe nvidia
modprobe nvidia_uvm
modprobe nvidia_drm modeset=1
sleep 2

if [ -c /dev/nvidia0 ]; then
    GPU_COUNT=$(ls /dev/nvidia[0-9]* 2>/dev/null | wc -l)
    echo "nvidia bereit: $GPU_COUNT GPU(s) verfuegbar"
else
    echo "WARNUNG: /dev/nvidia0 nicht da — nvidia-Probe vermutlich fehlgeschlagen"
    exit 1
fi
SCRIPT
chmod +x /usr/local/bin/egpu-gpu-init.sh

# --- systemd Unit (ruft externes Script auf) ---
cat > /etc/systemd/system/egpu-gpu-init.service << 'UNIT'
[Unit]
Description=eGPU GPU Init (Thunderbolt Link Training + nvidia modprobe)
After=bolt.service
Wants=bolt.service
Before=nvidia-persistenced.service egpu-managerd.service
ConditionPathExists=/sys/bus/thunderbolt

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/local/bin/egpu-gpu-init.sh

[Install]
WantedBy=multi-user.target
UNIT

log "  egpu-gpu-init.service erstellt"

# ─── 4. udev-Regel: Trigger bei eGPU Hot-Plug ───────────────────────────────
log "4/6: udev-Regel fuer eGPU Hot-Plug..."

cat > /etc/udev/rules.d/99-egpu-thunderbolt.rules << 'UDEV'
# Wenn die eGPU (RTX 5070 Ti) auf dem Bus erscheint, nvidia-Module laden
ACTION=="add", SUBSYSTEM=="pci", KERNEL=="0000:05:00.0", ATTR{vendor}=="0x10de", \
    TAG+="systemd", ENV{SYSTEMD_WANTS}+="egpu-gpu-init.service"
UDEV

udevadm control --reload-rules
log "  /etc/udev/rules.d/99-egpu-thunderbolt.rules erstellt"

# ─── 5. egpu-managerd.service — Nach gpu-init starten ────────────────────────
log "5/6: egpu-managerd.service aktualisieren..."

cat > /etc/systemd/system/egpu-managerd.service << 'UNIT'
[Unit]
Description=eGPU Manager Daemon (Rust)
Documentation=file:///home/janpow/Projekte/egpu/egpu-manager-spezifikation.md
After=network.target bolt.service nvidia-persistenced.service egpu-gpu-init.service
Wants=egpu-gpu-init.service

[Service]
Type=simple
ExecStartPre=/bin/sleep 3
ExecStart=/usr/local/bin/egpu-managerd --config /etc/egpu-manager/config.toml
Restart=on-failure
RestartSec=10
WatchdogSec=60
TimeoutStopSec=30
KillSignal=SIGTERM
StandardOutput=journal
StandardError=journal
SyslogIdentifier=egpu-managerd
Environment=RUST_LOG=info
ProtectSystem=strict
ReadWritePaths=/var/lib/egpu-manager /etc/egpu-manager
ProtectHome=read-only
PrivateTmp=false
NoNewPrivileges=yes
CapabilityBoundingSet=CAP_SYS_ADMIN CAP_NET_ADMIN
AmbientCapabilities=CAP_SYS_ADMIN CAP_NET_ADMIN
MemoryMax=2G
LimitNOFILE=65536
SupplementaryGroups=docker

[Install]
WantedBy=multi-user.target
UNIT

log "  egpu-managerd.service aktualisiert"

# ─── 6. nvidia-persistenced fix + systemd reload ────────────────────────────
log "6/6: nvidia-persistenced Drop-In + systemd reload..."

mkdir -p /etc/systemd/system/nvidia-persistenced.service.d

cat > /etc/systemd/system/nvidia-persistenced.service.d/wait-for-device.conf << 'OVERRIDE'
[Unit]
After=egpu-gpu-init.service
Wants=egpu-gpu-init.service

[Service]
Restart=on-failure
RestartSec=5
OVERRIDE

log "  nvidia-persistenced wartet auf egpu-gpu-init.service"

systemctl daemon-reload
systemctl enable egpu-gpu-init.service 2>/dev/null || true
log "  systemd reload + egpu-gpu-init enabled"

echo ""
echo "============================================="
echo -e "${GREEN}Alle Fixes angewendet!${NC}"
echo ""
echo "Was geaendert wurde:"
echo "  1. GRUB: iommu=pt pcie_aspm=off pcie_port_pm=off pci=realloc,assign-busses"
echo "     (pci=noacs ENTFERNT — war ungueltig auf stock-Kernel!)"
echo "  2. nvidia-Module beim Boot BLACKLISTED"
echo "  3. NEU: egpu-gpu-init.service"
echo "     -> Wartet auf TB-Link-Training"
echo "     -> Entfernt Ghost-Devices (0xFFFF)"
echo "     -> Laedt nvidia-Module erst wenn Link steht"
echo "  4. udev-Regel: Triggert gpu-init bei eGPU Hot-Plug"
echo "  5. egpu-managerd startet nach gpu-init"
echo "  6. nvidia-persistenced wartet auf gpu-init"
echo ""
echo "Boot-Reihenfolge (NEU):"
echo "  bolt.service -> egpu-gpu-init.service -> nvidia-persistenced -> egpu-managerd"
echo ""
echo -e "${YELLOW}Jetzt:${NC}"
echo "  sudo poweroff"
echo ""
echo "  15 Sekunden warten, dann einschalten."
echo "  KEIN reboot — Kaltstart noetig wegen MalfTLP + Xid 154!"
echo "============================================="
