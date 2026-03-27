#!/usr/bin/env bash
set -uo pipefail
# Kein `set -e` — Tests dürfen fehlschlagen ohne das Skript abzubrechen

# ============================================================================
# test-evo-x2.sh — EVO-X2 Setup- und Connection-Tests
# ============================================================================
#
# Prüft ob die EVO-X2 korrekt eingerichtet ist und alle Dienste erreichbar.
# Aufruf: bash test-evo-x2.sh
#
# Exit-Codes:
#   0 — Alle Tests bestanden
#   1 — Mindestens ein Test fehlgeschlagen

# ---------------------------------------------------------------------------
# Konfiguration
# ---------------------------------------------------------------------------
EVO_IP="${EVO_IP:-192.168.178.72}"
EVO_USER="${EVO_USER:-janpow}"
EVO_HOST="$EVO_USER@$EVO_IP"

SSH_OPTS="-o ConnectTimeout=5 -o StrictHostKeyChecking=accept-new -o BatchMode=yes"

# Erwartete Modelle auf EVO-X2
EXPECTED_MODELS=(
    "bge-m3"
    "qllama/bge-reranker-v2-m3"
    "qwen2.5:72b-instruct-q4_K_M"
    "huihui_ai/qwen2.5-abliterate:32b-instruct"
)

# Farben
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# Zähler
PASS=0
FAIL=0
WARN=0
SKIP=0

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
pass()  { ((PASS++)); echo -e "  ${GREEN}✓ PASS${NC}  $*"; }
fail()  { ((FAIL++)); echo -e "  ${RED}✗ FAIL${NC}  $*"; }
warn()  { ((WARN++)); echo -e "  ${YELLOW}⚠ WARN${NC}  $*"; }
skip()  { ((SKIP++)); echo -e "  ${CYAN}– SKIP${NC}  $*"; }
header() { echo -e "\n${BOLD}── $* ──${NC}"; }

evo() {
    ssh $SSH_OPTS "$EVO_HOST" "$@" 2>/dev/null
}

# ---------------------------------------------------------------------------
# T1: SSH-Konnektivität
# ---------------------------------------------------------------------------
test_ssh() {
    header "T1: SSH-Konnektivität"

    if evo "echo ok" | grep -q ok; then
        pass "SSH-Verbindung zu $EVO_HOST"
    else
        fail "SSH-Verbindung zu $EVO_HOST — alle weiteren Tests übersprungen"
        return 1
    fi

    # SSH-Key (kein Passwort nötig)
    if ssh $SSH_OPTS -o PasswordAuthentication=no "$EVO_HOST" "echo ok" 2>/dev/null | grep -q ok; then
        pass "SSH-Key-Auth (passwortlos)"
    else
        warn "SSH-Key-Auth nicht eingerichtet — Passwort nötig"
    fi
}

# ---------------------------------------------------------------------------
# T2: System-Grundlagen
# ---------------------------------------------------------------------------
test_system() {
    header "T2: System-Grundlagen"

    # Hostname
    local hostname
    hostname=$(evo "hostname")
    if [[ -n "$hostname" ]]; then
        pass "Hostname: $hostname"
    else
        fail "Hostname nicht abrufbar"
    fi

    # Kernel
    local kernel
    kernel=$(evo "uname -r")
    pass "Kernel: $kernel"

    # Disk: genug Platz für Modelle? (mindestens 200 GB frei)
    local avail_gb
    avail_gb=$(evo "df --output=avail / | tail -1 | awk '{printf \"%.0f\", \$1/1024/1024}'")
    if [[ "$avail_gb" -ge 200 ]]; then
        pass "Festplatte: ${avail_gb} GB frei (≥200 GB)"
    elif [[ "$avail_gb" -ge 50 ]]; then
        warn "Festplatte: nur ${avail_gb} GB frei — LVM erweitern empfohlen"
    else
        fail "Festplatte: nur ${avail_gb} GB frei — zu wenig für Modelle"
    fi

    # RAM
    local total_gb
    total_gb=$(evo "free -g | awk '/Mem:/{print \$2}'")
    if [[ "$total_gb" -ge 100 ]]; then
        pass "RAM: ${total_gb} GB (≥100 GB)"
    else
        warn "RAM: nur ${total_gb} GB"
    fi

    # Kernel-Version (Strix Halo braucht ≥6.15)
    local kernel
    kernel=$(evo "uname -r")
    local kernel_major
    kernel_major=$(echo "$kernel" | cut -d. -f1-2)
    if awk "BEGIN{exit !($kernel_major >= 6.15)}"; then
        pass "Kernel $kernel (≥6.15, Strix Halo Support)"
    else
        fail "Kernel $kernel — zu alt für Strix Halo (braucht ≥6.15). HWE-Kernel installieren: sudo apt install linux-generic-hwe-24.04"
    fi

    # amdgpu Treiber geladen UND gebunden?
    if evo "lsmod | grep -q amdgpu"; then
        pass "amdgpu-Kernelmodul geladen"
    else
        fail "amdgpu-Kernelmodul nicht geladen"
    fi

    local gpu_driver
    gpu_driver=$(evo "lspci -s c5:00.0 -k 2>/dev/null | grep 'Kernel driver in use' | awk '{print \$NF}'" || echo "")
    if [[ "$gpu_driver" == "amdgpu" ]]; then
        pass "amdgpu-Treiber an GPU gebunden"
    else
        fail "amdgpu-Treiber NICHT an GPU gebunden (driver: '${gpu_driver:-none}') — Kernel zu alt oder Firmware fehlt"
    fi

    # GTT (AMD GPU-Speicher) — verschiedene sysfs-Pfade prüfen
    local gtt_bytes
    gtt_bytes=$(evo "for f in /sys/class/drm/card*/device/mem_info_gtt_total; do cat \"\$f\" 2>/dev/null && break; done" || echo "0")
    if [[ -n "$gtt_bytes" && "$gtt_bytes" != "0" ]]; then
        local gtt_gb=$((gtt_bytes / 1024 / 1024 / 1024))
        pass "GTT: ${gtt_gb} GB"
    else
        fail "GTT nicht verfügbar — amdgpu nicht korrekt gebunden"
    fi
}

# ---------------------------------------------------------------------------
# T3: Ollama
# ---------------------------------------------------------------------------
test_ollama() {
    header "T3: Ollama"

    # Ollama installiert?
    if evo "command -v ollama" &>/dev/null; then
        local ver
        ver=$(evo "ollama --version" 2>/dev/null || echo "?")
        pass "Ollama installiert: $ver"
    else
        fail "Ollama nicht installiert"
        return
    fi

    # Ollama-Service aktiv?
    local status
    status=$(evo "systemctl is-active ollama 2>/dev/null || echo inactive")
    if [[ "$status" == "active" ]]; then
        pass "Ollama-Service: active"
    else
        fail "Ollama-Service: $status"
        return
    fi

    # API lokal erreichbar?
    if evo "curl -sf http://localhost:11434/api/tags" &>/dev/null; then
        pass "Ollama API lokal erreichbar (localhost:11434)"
    else
        fail "Ollama API lokal nicht erreichbar"
    fi

    # API vom NUC erreichbar? (0.0.0.0 Binding)
    if curl -sf --connect-timeout 5 "http://$EVO_IP:11434/api/tags" &>/dev/null; then
        pass "Ollama API vom NUC erreichbar (http://$EVO_IP:11434)"
    else
        fail "Ollama API vom NUC NICHT erreichbar — OLLAMA_HOST=0.0.0.0?"
    fi

    # Modelle prüfen
    local models_json
    models_json=$(curl -sf --connect-timeout 5 "http://$EVO_IP:11434/api/tags" 2>/dev/null || echo '{"models":[]}')

    for expected in "${EXPECTED_MODELS[@]}"; do
        if echo "$models_json" | grep -q "$expected"; then
            pass "Modell vorhanden: $expected"
        else
            fail "Modell fehlt: $expected"
        fi
    done
}

# ---------------------------------------------------------------------------
# T4: Ollama Inferenz-Test
# ---------------------------------------------------------------------------
test_ollama_inference() {
    header "T4: Ollama Inferenz"

    # Schneller Test mit dem kleinsten geladenen Modell (bge-m3 Embedding)
    local embed_result
    embed_result=$(curl -sf --connect-timeout 10 --max-time 30 \
        "http://$EVO_IP:11434/api/embed" \
        -d '{"model":"bge-m3","input":"test embedding"}' 2>/dev/null)

    if echo "$embed_result" | grep -q "embeddings"; then
        pass "Embedding-Inferenz (bge-m3): OK"
    else
        fail "Embedding-Inferenz (bge-m3): fehlgeschlagen"
    fi

    # Chat-Test mit qwen2.5:72b (kurze Antwort, max 10 Tokens)
    local chat_result
    chat_result=$(curl -sf --connect-timeout 10 --max-time 120 \
        "http://$EVO_IP:11434/api/chat" \
        -d '{"model":"qwen2.5:72b-instruct-q4_K_M","messages":[{"role":"user","content":"Say OK"}],"stream":false,"options":{"num_predict":10}}' 2>/dev/null)

    if echo "$chat_result" | grep -q "message"; then
        pass "Chat-Inferenz (qwen2.5:72b): OK"
    else
        # Könnte Cold-Start sein (>60s für 47 GB in GTT)
        warn "Chat-Inferenz (qwen2.5:72b): Timeout oder Fehler — Cold-Start?"
    fi

    # Abliterate-32B Test (love-ai Modell, passt parallel mit 72B in GTT)
    local models_json
    models_json=$(curl -sf --connect-timeout 5 "http://$EVO_IP:11434/api/tags" 2>/dev/null || echo "")
    if echo "$models_json" | grep -q "abliterate"; then
        local abl_result
        abl_result=$(curl -sf --connect-timeout 10 --max-time 60 \
            "http://$EVO_IP:11434/api/chat" \
            -d '{"model":"huihui_ai/qwen2.5-abliterate:32b-instruct","messages":[{"role":"user","content":"Say OK"}],"stream":false,"options":{"num_predict":10}}' 2>/dev/null)

        if echo "$abl_result" | grep -q "message"; then
            pass "Chat-Inferenz (abliterate-32b): OK"
        else
            warn "Chat-Inferenz (abliterate-32b): Timeout oder Fehler"
        fi
    else
        skip "abliterate-32b nicht installiert — übersprungen"
    fi

    # Parallel-Test: Prüfe ob beide Modelle gleichzeitig geladen sind
    local running
    running=$(curl -sf --connect-timeout 5 "http://$EVO_IP:11434/api/ps" 2>/dev/null || echo "")
    local running_count
    running_count=$(echo "$running" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('models',[])))" 2>/dev/null || echo "0")
    if [[ "$running_count" -ge 2 ]]; then
        pass "Parallel-Betrieb: $running_count Modelle gleichzeitig geladen"
    elif [[ "$running_count" -eq 1 ]]; then
        warn "Nur 1 Modell geladen (KEEP_ALIVE=-1 aktiv? Beide Modelle einmal anfragen)"
    else
        warn "Keine Modelle geladen"
    fi
}

# ---------------------------------------------------------------------------
# T5: evo-x2-services
# ---------------------------------------------------------------------------
test_services() {
    header "T5: evo-x2-services"

    # Binary vorhanden?
    if evo "test -x ~/.local/bin/evo-x2-services"; then
        pass "Binary: ~/.local/bin/evo-x2-services"
    else
        skip "evo-x2-services nicht deployed — Phase 'services' noch nicht gelaufen"
        return
    fi

    # evo-metrics Service
    local svc_status
    svc_status=$(evo "systemctl is-active evo-metrics 2>/dev/null || echo inactive")
    if [[ "$svc_status" == "active" ]]; then
        pass "evo-metrics Service: active"
    else
        fail "evo-metrics Service: $svc_status"
    fi

    # Metrics-Endpoint lokal
    if evo "curl -sf http://localhost:8084/health" &>/dev/null; then
        pass "Metrics Health-Endpoint lokal (Port 8084)"
    else
        fail "Metrics Health-Endpoint lokal nicht erreichbar"
    fi

    # Metrics-Endpoint vom NUC
    if curl -sf --connect-timeout 5 "http://$EVO_IP:8084/health" &>/dev/null; then
        pass "Metrics Health vom NUC erreichbar"
    else
        fail "Metrics Health vom NUC NICHT erreichbar"
    fi

    # Metrics-Daten prüfen
    local metrics
    metrics=$(curl -sf --connect-timeout 5 "http://$EVO_IP:8084/metrics" 2>/dev/null || echo "")
    if echo "$metrics" | grep -q "gtt"; then
        pass "Metrics-Endpoint liefert GTT-Daten"
    else
        fail "Metrics-Endpoint liefert keine GTT-Daten"
    fi

    if echo "$metrics" | grep -q "cpu_load"; then
        pass "Metrics-Endpoint liefert CPU-Daten"
    else
        fail "Metrics-Endpoint liefert keine CPU-Daten"
    fi

    # Neue Metrics-Felder (Widget-Optimierung)
    if echo "$metrics" | grep -q "ollama"; then
        pass "Metrics-Endpoint liefert Ollama-Daten"
    else
        warn "Metrics-Endpoint ohne Ollama-Daten (alter Binary?)"
    fi

    if echo "$metrics" | grep -q "temperature_c"; then
        pass "Metrics-Endpoint liefert GPU-Temperatur"
    else
        warn "Metrics-Endpoint ohne GPU-Temperatur"
    fi

    if echo "$metrics" | grep -q "tailscale"; then
        pass "Metrics-Endpoint liefert Tailscale-Daten"
    else
        warn "Metrics-Endpoint ohne Tailscale-Daten"
    fi

    # Webhook Service
    svc_status=$(evo "systemctl is-active evo-webhook 2>/dev/null || echo inactive")
    if [[ "$svc_status" == "active" ]]; then
        pass "evo-webhook Service: active"
    else
        warn "evo-webhook Service: $svc_status"
    fi
}

# ---------------------------------------------------------------------------
# T6: GTK-Widget Konfiguration (NUC-seitig)
# ---------------------------------------------------------------------------
test_widget_config() {
    header "T6: GTK-Widget Konfiguration (NUC)"

    local config_file="$HOME/.config/evo-manager/config.json"
    if [[ -f "$config_file" ]]; then
        pass "Config existiert: $config_file"
    else
        fail "Config fehlt: $config_file"
        return
    fi

    # IP korrekt?
    local configured_ip
    configured_ip=$(python3 -c "import json; print(json.load(open('$config_file')).get('evo_ip',''))" 2>/dev/null || echo "")
    if [[ "$configured_ip" == "$EVO_IP" ]]; then
        pass "Widget-Config evo_ip: $configured_ip"
    elif [[ -z "$configured_ip" ]]; then
        fail "Widget-Config evo_ip ist leer"
    else
        warn "Widget-Config evo_ip=$configured_ip (erwartet: $EVO_IP)"
    fi

    # Widget-Prozess läuft?
    if pgrep -f evo-manager-widget &>/dev/null; then
        pass "evo-manager-widget Prozess läuft"
    else
        warn "evo-manager-widget Prozess nicht gestartet"
    fi
}

# ---------------------------------------------------------------------------
# T7: love-ai Konfiguration
# ---------------------------------------------------------------------------
test_loveai_config() {
    header "T7: love-ai Konfiguration"

    local env_file="$HOME/Projekte/x_chat/love-ai/api/.env"
    if [[ ! -f "$env_file" ]]; then
        skip "love-ai .env nicht gefunden: $env_file"
        return
    fi

    pass ".env existiert: $env_file"

    # OLLAMA_URL zeigt auf EVO-X2? (LAN-IP oder Tailscale-IP)
    local ollama_url
    ollama_url=$(grep "^OLLAMA_URL=" "$env_file" | cut -d= -f2- || echo "")
    if echo "$ollama_url" | grep -qE "($EVO_IP|100\.81\.4\.99)"; then
        pass "OLLAMA_URL zeigt auf EVO-X2: $ollama_url"
    else
        fail "OLLAMA_URL zeigt NICHT auf EVO-X2: $ollama_url"
    fi

    # Unzensiertes Modell konfiguriert?
    local chat_model
    chat_model=$(grep "^OLLAMA_CHAT_MODEL=" "$env_file" | cut -d= -f2- || echo "")
    if echo "$chat_model" | grep -qiE "dolphin|abliterate|uncensored"; then
        pass "OLLAMA_CHAT_MODEL (unzensiert): $chat_model"
    else
        warn "OLLAMA_CHAT_MODEL nicht als unzensiert erkannt: $chat_model"
    fi

    # Embedding-Modell?
    local embed_model
    embed_model=$(grep "^OLLAMA_EMBED_MODEL=" "$env_file" | cut -d= -f2- || echo "")
    if [[ -n "$embed_model" ]]; then
        pass "OLLAMA_EMBED_MODEL: $embed_model"
    else
        warn "OLLAMA_EMBED_MODEL nicht gesetzt"
    fi
}

# ---------------------------------------------------------------------------
# T8: config.toml LLM-Gateway
# ---------------------------------------------------------------------------
test_gateway_config() {
    header "T8: config.toml LLM-Gateway"

    local config="$HOME/Projekte/EGPU_EVO-X2_Manager/config.toml"
    if [[ ! -f "$config" ]]; then
        skip "config.toml nicht gefunden"
        return
    fi

    # EVO-X2 IP eingetragen (kein TAILSCALE_IP Platzhalter)?
    if grep -q "TAILSCALE_IP" "$config"; then
        fail "config.toml enthält noch TAILSCALE_IP Platzhalter"
    else
        pass "Keine TAILSCALE_IP Platzhalter mehr"
    fi

    # Korrekte Modellnamen (keine qwen3:72b oder dolphin3:72b)?
    if grep -q '"qwen3:72b"' "$config"; then
        fail "config.toml enthält noch qwen3:72b (existiert nicht)"
    else
        pass "Keine falschen qwen3:72b Referenzen"
    fi

    if grep -q '"dolphin3:72b"' "$config"; then
        fail "config.toml enthält noch dolphin3:72b (existiert nicht)"
    else
        pass "Keine falschen dolphin3:72b Referenzen"
    fi

    # love-ai Routing korrekt?
    if grep -A3 'app_id = "love-ai"' "$config" | grep -q "ollama-evo-x2"; then
        pass "love-ai Routing → ollama-evo-x2"
    else
        fail "love-ai Routing fehlt oder falsch"
    fi
}

# ---------------------------------------------------------------------------
# T9: Tailscale (optional)
# ---------------------------------------------------------------------------
test_tailscale() {
    header "T9: Tailscale (optional)"

    if evo "command -v tailscale" &>/dev/null; then
        pass "Tailscale installiert"
        local ts_status
        ts_status=$(evo "tailscale status --json 2>/dev/null | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get(\"BackendState\",\"?\"))'" 2>/dev/null || echo "?")
        if [[ "$ts_status" == "Running" ]]; then
            local ts_ip
            ts_ip=$(evo "tailscale ip -4" 2>/dev/null || echo "?")
            pass "Tailscale aktiv: $ts_ip"
        else
            warn "Tailscale nicht verbunden (Status: $ts_status)"
        fi
    else
        skip "Tailscale nicht installiert"
    fi
}

# ---------------------------------------------------------------------------
# T10: End-to-End NUC → EVO-X2 → Ollama
# ---------------------------------------------------------------------------
test_e2e() {
    header "T10: End-to-End (NUC → EVO-X2 → Ollama)"

    # Kann der NUC direkt ein Embedding auf der EVO-X2 erzeugen?
    local result
    result=$(curl -sf --connect-timeout 5 --max-time 30 \
        "http://$EVO_IP:11434/api/embed" \
        -d '{"model":"bge-m3","input":"EVO-X2 end-to-end test from NUC"}' 2>/dev/null)

    if echo "$result" | grep -q "embeddings"; then
        pass "NUC → EVO-X2 Ollama Embedding: OK"
    else
        fail "NUC → EVO-X2 Ollama Embedding: fehlgeschlagen"
    fi

    # Round-Trip-Latenz
    local start_ms end_ms latency_ms
    start_ms=$(date +%s%3N)
    curl -sf --connect-timeout 5 --max-time 10 "http://$EVO_IP:11434/api/tags" &>/dev/null
    end_ms=$(date +%s%3N)
    latency_ms=$((end_ms - start_ms))

    if [[ "$latency_ms" -lt 100 ]]; then
        pass "API Round-Trip Latenz: ${latency_ms}ms (<100ms)"
    elif [[ "$latency_ms" -lt 500 ]]; then
        pass "API Round-Trip Latenz: ${latency_ms}ms (<500ms)"
    else
        warn "API Round-Trip Latenz: ${latency_ms}ms (>500ms — Netzwerk langsam?)"
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
echo ""
echo -e "${BOLD}╔═══════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║         EVO-X2 Test-Suite                                ║${NC}"
echo -e "${BOLD}║         Ziel: $EVO_HOST $(printf '%-24s' "$EVO_IP")     ║${NC}"
echo -e "${BOLD}║         $(date '+%Y-%m-%d %H:%M:%S')                              ║${NC}"
echo -e "${BOLD}╚═══════════════════════════════════════════════════════════╝${NC}"

# T1 muss bestehen, sonst abbrechen
if ! test_ssh; then
    echo ""
    echo -e "${RED}SSH nicht erreichbar — Tests abgebrochen.${NC}"
    exit 1
fi

test_system
test_ollama
test_ollama_inference
test_services
test_widget_config
test_loveai_config
test_gateway_config
test_tailscale
test_e2e

# ---------------------------------------------------------------------------
# Zusammenfassung
# ---------------------------------------------------------------------------
TOTAL=$((PASS + FAIL + WARN + SKIP))
echo ""
echo -e "${BOLD}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BOLD}  Ergebnis: $TOTAL Tests${NC}"
echo -e "  ${GREEN}✓ $PASS bestanden${NC}"
if [[ $FAIL -gt 0 ]]; then
    echo -e "  ${RED}✗ $FAIL fehlgeschlagen${NC}"
fi
if [[ $WARN -gt 0 ]]; then
    echo -e "  ${YELLOW}⚠ $WARN Warnungen${NC}"
fi
if [[ $SKIP -gt 0 ]]; then
    echo -e "  ${CYAN}– $SKIP übersprungen${NC}"
fi
echo -e "${BOLD}═══════════════════════════════════════════════════════════${NC}"

if [[ $FAIL -gt 0 ]]; then
    echo -e "\n${RED}FEHLGESCHLAGEN — $FAIL Test(s) nicht bestanden.${NC}"
    exit 1
else
    echo -e "\n${GREEN}BESTANDEN — alle kritischen Tests OK.${NC}"
    exit 0
fi
