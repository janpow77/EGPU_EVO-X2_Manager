#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# setup-evo-x2.sh — Vollständiges EVO-X2 Setup vom NUC aus
# ============================================================================
#
# Aufruf:  bash setup-evo-x2.sh [PHASE]
#
# Phasen:
#   all       — Alles (Default)
#   keys      — SSH-Key hinterlegen
#   base      — Grundpakete + Verzeichnisse
#   tailscale — Tailscale VPN
#   ollama    — Ollama + Modelle
#   models    — Nur Modelle nachziehen
#   services  — evo-x2-services deployen
#   verify    — Health-Check aller Dienste
#
# Beispiel:
#   bash setup-evo-x2.sh              # Alles
#   bash setup-evo-x2.sh ollama       # Nur Ollama + Modelle
#   bash setup-evo-x2.sh models       # Nur fehlende Modelle nachziehen
#   bash setup-evo-x2.sh verify       # Status prüfen

# ---------------------------------------------------------------------------
# Konfiguration
# ---------------------------------------------------------------------------
EVO_IP="${EVO_IP:-192.168.178.72}"
EVO_USER="${EVO_USER:-janpow}"
EVO_HOST="$EVO_USER@$EVO_IP"
PHASE="${1:-all}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# SSH-Optionen (ServerAlive hält Verbindung bei langen Downloads)
SSH_OPTS="-o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new -o ServerAliveInterval=60 -o ServerAliveCountMax=10"

# Farben
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
info()  { echo -e "${CYAN}[INFO]${NC}  $*"; }
ok()    { echo -e "${GREEN}[OK]${NC}    $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
fail()  { echo -e "${RED}[FAIL]${NC}  $*"; }

evo() {
    # SSH-Befehl auf EVO-X2 ausführen (non-interactive)
    ssh $SSH_OPTS "$EVO_HOST" "$@"
}

evo_tty() {
    # SSH mit PTY (für sudo-Passwort und interaktive Installer)
    ssh -t $SSH_OPTS "$EVO_HOST" "$@"
}

header() {
    echo ""
    echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${CYAN}  $*${NC}"
    echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
    echo ""
}

check_ssh() {
    if ! evo "echo ok" &>/dev/null; then
        fail "Kann $EVO_HOST nicht erreichen. SSH erreichbar?"
        exit 1
    fi
    ok "SSH-Verbindung zu $EVO_HOST steht"
}

# ---------------------------------------------------------------------------
# Phase: SSH-Keys
# ---------------------------------------------------------------------------
phase_keys() {
    header "Phase 1: SSH-Key hinterlegen"

    if evo "echo ok" &>/dev/null; then
        ok "SSH-Zugang funktioniert bereits"
    else
        info "Hinterlege SSH-Key auf $EVO_HOST ..."
        ssh-copy-id "$EVO_HOST"
        ok "SSH-Key hinterlegt"
    fi
}

# ---------------------------------------------------------------------------
# Phase: Grundsystem
# ---------------------------------------------------------------------------
phase_base() {
    header "Phase 2: Grundsystem einrichten"
    check_ssh

    info "System-Updates ..."
    evo_tty "sudo apt update"
    evo_tty "sudo apt upgrade -y"

    info "Installiere Grundpakete ..."
    evo_tty "sudo apt install -y build-essential curl git wget htop tmux net-tools jq unzip software-properties-common linux-firmware"

    # HWE-Kernel für Strix Halo (RDNA 3.5) — Kernel 6.8 hat keinen Support (Fatal GPU init error)
    local current_kernel
    current_kernel=$(evo "uname -r")
    if [[ "$current_kernel" == 6.8.* ]]; then
        info "Kernel $current_kernel zu alt für Strix Halo — installiere HWE-Kernel ..."
        evo_tty "sudo apt install -y linux-generic-hwe-24.04"
        warn "HWE-Kernel installiert — REBOOT NÖTIG: ssh $EVO_HOST 'sudo reboot'"
    else
        ok "Kernel $current_kernel — Strix Halo Support vorhanden"
    fi

    info "Erstelle Verzeichnisse ..."
    evo "mkdir -p ~/.local/bin ~/.config/evo-x2"

    # ~/.local/bin in PATH sicherstellen
    evo 'grep -q "\.local/bin" ~/.bashrc || echo "export PATH=\"\$HOME/.local/bin:\$PATH\"" >> ~/.bashrc'

    ok "Grundsystem eingerichtet"
}

# ---------------------------------------------------------------------------
# Phase: Tailscale
# ---------------------------------------------------------------------------
phase_tailscale() {
    header "Phase 3: Tailscale VPN"
    check_ssh

    if evo "command -v tailscale" &>/dev/null; then
        ok "Tailscale bereits installiert"
        info "Status:"
        evo "tailscale status" || warn "Tailscale nicht verbunden"
    else
        info "Installiere Tailscale ..."
        evo_tty "curl -fsSL https://tailscale.com/install.sh | sh"
        info "Starte Tailscale — Browser-Auth nötig:"
        evo_tty "sudo tailscale up"
    fi
}

# ---------------------------------------------------------------------------
# Phase: Ollama
# ---------------------------------------------------------------------------
phase_ollama() {
    header "Phase 4: Ollama installieren"
    check_ssh

    if evo "command -v ollama" &>/dev/null; then
        ok "Ollama bereits installiert"
        evo "ollama --version"
    else
        info "Installiere Ollama ..."
        evo_tty "curl -fsSL https://ollama.com/install.sh | sh"
    fi

    # Sicherstellen dass Ollama läuft
    evo_tty "sudo systemctl enable --now ollama"
    sleep 3

    if evo "curl -sf http://localhost:11434/api/tags" &>/dev/null; then
        ok "Ollama API erreichbar"
    else
        fail "Ollama API nicht erreichbar"
        return 1
    fi

    # Ollama auf alle Interfaces binden (für NUC-Zugriff)
    info "Konfiguriere Ollama für Netzwerkzugriff ..."
    evo_tty "sudo mkdir -p /etc/systemd/system/ollama.service.d"

    # Override-Datei per scp statt verschachteltem Heredoc
    local TMPFILE
    TMPFILE=$(mktemp)
    cat > "$TMPFILE" << 'OVERRIDE'
[Service]
Environment=OLLAMA_HOST=0.0.0.0
Environment=OLLAMA_KEEP_ALIVE=-1
OVERRIDE
    scp $SSH_OPTS "$TMPFILE" "$EVO_HOST:/tmp/ollama-override.conf"
    rm -f "$TMPFILE"
    evo_tty "sudo mv /tmp/ollama-override.conf /etc/systemd/system/ollama.service.d/override.conf"
    evo_tty "sudo systemctl daemon-reload"
    evo_tty "sudo systemctl restart ollama"
    sleep 3

    phase_models
}

phase_models() {
    header "Phase 4b: Modelle laden"
    check_ssh

    info "Lade Modelle — das dauert bei 72B-Modellen eine Weile ..."
    echo ""

    # ---- Embeddings + Reranker (klein, schnell) ----
    info "[1/4] bge-m3 — Embeddings (1024d, ~1.2 GB) ..."
    evo "ollama pull bge-m3" && ok "bge-m3" || fail "bge-m3"

    info "[2/4] bge-reranker-v2-m3 — Reranker (~636 MB) ..."
    evo "ollama pull qllama/bge-reranker-v2-m3" && ok "bge-reranker-v2-m3" || fail "bge-reranker-v2-m3"

    # ---- Qwen2.5-72B (RAG, Checklisten, Rechnungen) ----
    info "[3/4] qwen3:32b — Haupt-LLM (~47 GB) ..."
    evo "ollama pull qwen3:32b" && ok "qwen3:32b" || fail "qwen3:32b"

    # ---- Qwen2.5-32B abliterated (love-ai, unzensiert, parallel mit 72B) ----
    info "[4/4] qwen2.5-abliterate:32b — Unzensiertes RP-LLM (~20 GB) ..."
    evo "ollama pull huihui_ai/qwen2.5-abliterate:32b-instruct" && ok "qwen2.5-abliterate:32b" || fail "qwen2.5-abliterate:32b"

    echo ""
    info "Installierte Modelle:"
    evo "ollama list"
}

# ---------------------------------------------------------------------------
# Phase: evo-x2-services
# ---------------------------------------------------------------------------
phase_services() {
    header "Phase 5: evo-x2-services deployen"
    check_ssh

    info "Baue evo-x2-services (Release) ..."
    cd "$SCRIPT_DIR"
    cargo build --release -p evo-x2-services

    BIN="$SCRIPT_DIR/target/release/evo-x2-services"
    if [[ ! -f "$BIN" ]]; then
        fail "Binary nicht gefunden: $BIN"
        return 1
    fi

    info "Kopiere Binary auf EVO-X2 ..."
    evo "mkdir -p ~/.local/bin"
    scp $SSH_OPTS "$BIN" "$EVO_HOST:~/.local/bin/evo-x2-services"
    ok "Binary deployed"

    # systemd-Units per scp (sauberer als verschachteltes Heredoc über SSH)
    info "Erstelle systemd-Units ..."

    local TMPDIR
    TMPDIR=$(mktemp -d)

    cat > "$TMPDIR/evo-metrics.service" << EOF
[Unit]
Description=EVO-X2 Metrics Server
After=network.target

[Service]
Type=simple
User=$EVO_USER
ExecStart=/home/$EVO_USER/.local/bin/evo-x2-services metrics
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

    cat > "$TMPDIR/evo-webhook.service" << EOF
[Unit]
Description=EVO-X2 GitHub Webhook
After=network.target

[Service]
Type=simple
User=$EVO_USER
ExecStart=/home/$EVO_USER/.local/bin/evo-x2-services webhook
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

    scp $SSH_OPTS "$TMPDIR/evo-metrics.service" "$TMPDIR/evo-webhook.service" "$EVO_HOST:/tmp/"
    rm -rf "$TMPDIR"

    evo_tty "sudo mv /tmp/evo-metrics.service /tmp/evo-webhook.service /etc/systemd/system/"
    evo_tty "sudo systemctl daemon-reload"
    evo_tty "sudo systemctl enable --now evo-metrics evo-webhook"

    ok "Services deployed und gestartet"
}

# ---------------------------------------------------------------------------
# Phase: Verifizierung
# ---------------------------------------------------------------------------
phase_verify() {
    header "Phase 6: Verifizierung"
    check_ssh

    echo "--- System ---"
    evo "hostname && uname -r"
    evo "free -h | head -2"
    echo ""

    echo "--- GPU / GTT ---"
    evo "cat /sys/class/drm/card*/device/mem_info_gtt_total 2>/dev/null || echo 'kein GTT gefunden'"
    echo ""

    echo "--- Ollama ---"
    if evo "curl -sf http://localhost:11434/api/tags" &>/dev/null; then
        ok "Ollama API (Port 11434)"
        evo "ollama list"
    else
        fail "Ollama API nicht erreichbar"
    fi
    echo ""

    echo "--- Tailscale ---"
    if evo "tailscale status" &>/dev/null; then
        TAILSCALE_IP=$(evo "tailscale ip -4" 2>/dev/null || echo "?")
        ok "Tailscale verbunden: $TAILSCALE_IP"
    else
        warn "Tailscale nicht verbunden"
    fi
    echo ""

    echo "--- Services ---"
    for svc in evo-metrics evo-webhook ollama; do
        STATUS=$(evo "systemctl is-active $svc 2>/dev/null || echo inactive")
        if [[ "$STATUS" == "active" ]]; then
            ok "$svc: active"
        else
            warn "$svc: $STATUS"
        fi
    done
    echo ""

    echo "--- Netzwerk-Ports ---"
    for port in 11434 8084 9000; do
        if evo "curl -sf http://localhost:$port/health" &>/dev/null || \
           evo "curl -sf http://localhost:$port/api/tags" &>/dev/null; then
            ok "Port $port erreichbar"
        else
            warn "Port $port nicht erreichbar"
        fi
    done

    echo ""
    echo "--- Erreichbarkeit vom NUC ---"
    if curl -sf "http://$EVO_IP:11434/api/tags" &>/dev/null; then
        ok "Ollama von NUC erreichbar (http://$EVO_IP:11434)"
    else
        warn "Ollama vom NUC NICHT erreichbar — Firewall/Binding prüfen"
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
echo ""
echo -e "${CYAN}╔═══════════════════════════════════════════════════════════╗${NC}"
echo -e "${CYAN}║         EVO-X2 Setup — Remote vom NUC                   ║${NC}"
echo -e "${CYAN}║         Ziel: $EVO_HOST $(printf '%-24s' "$EVO_IP")    ║${NC}"
echo -e "${CYAN}╚═══════════════════════════════════════════════════════════╝${NC}"
echo ""

case "$PHASE" in
    all)
        phase_keys
        phase_base
        phase_tailscale
        phase_ollama
        phase_services
        phase_verify
        ;;
    keys)      phase_keys ;;
    base)      phase_base ;;
    tailscale) phase_tailscale ;;
    ollama)    phase_ollama ;;
    models)    phase_models ;;
    services)  phase_services ;;
    verify)    phase_verify ;;
    *)
        echo "Verwendung: bash setup-evo-x2.sh [all|keys|base|tailscale|ollama|models|services|verify]"
        exit 1
        ;;
esac

echo ""
ok "Phase '$PHASE' abgeschlossen."
