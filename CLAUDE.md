# CLAUDE.md — egpu-manager

## Projektbeschreibung

**egpu-manager** ist ein systemd-fähiger Daemon zur Verwaltung von zwei NVIDIA-GPUs (interne RTX 5060 Ti + externe eGPU RTX 5070 Ti über Thunderbolt). Der Daemon übernimmt:

- GPU-Monitoring (nvidia-smi, sysfs, /dev/kmsg)
- Automatisches Failover und 5-stufige Recovery-State-Machine
- VRAM-basiertes Workload-Scheduling für Docker-Compose-Pipelines
- LLM Gateway mit Multi-Provider-Routing (Ollama, Anthropic, Gemini)
- Remote-GPU-Unterstützung über separaten HTTP-Listener
- Echtzeit-Updates via SSE und eingebettete Web-UI

**Stack:** Rust (Edition 2024), Tokio, Axum 0.8, SQLite (rusqlite), GTK4, tracing, serde/TOML.

## Architekturübersicht

```
egpu/
├── Cargo.toml                    # Workspace-Definition (5 Crates)
├── config.toml                   # Daemon-Konfiguration (Schema v1)
├── pipeline-profiles.toml        # GPU-Pipeline-Analyseergebnisse
├── egpu-manager-spezifikation.md # Spezifikation v2.4 (kanonische Referenz)
├── llm-secrets.toml.example      # API-Key-Vorlage (nie einchecken!)
├── egpu-managerd.service         # systemd Unit-File
├── deploy.sh                     # Deployment-Helper
├── install-service.sh            # systemd-Installation
├── crates/
│   ├── egpu-managerd/            # Core-Daemon (Binary)
│   │   └── src/
│   │       ├── main.rs           # Entry-Point, Signal-Handling
│   │       ├── monitor.rs        # MonitorOrchestrator, GPU-Polling
│   │       ├── scheduler.rs      # VramScheduler, AdmissionState
│   │       ├── recovery.rs       # 5-stufige Recovery-State-Machine
│   │       ├── db.rs             # SQLite EventDb (Audit-Log)
│   │       ├── warning.rs        # WarningStateMachine (Green→Red)
│   │       ├── nvidia.rs         # nvidia-smi Wrapper
│   │       ├── ollama.rs         # OllamaControl
│   │       ├── docker.rs         # DockerComposeControl
│   │       ├── health_score.rs   # Composite Health Scoring
│   │       ├── kmsg.rs           # Kernel-Message-Monitoring
│   │       ├── aer.rs            # PCIe AER-Fehlererkennung
│   │       ├── link_health.rs    # PCIe-Link-Degradation
│   │       ├── sysfs.rs          # sysfs-Monitore
│   │       ├── sysinfo.rs        # Systeminformationen
│   │       ├── remote_listener.rs# Remote-GPU HTTP-Listener (Port 7843)
│   │       ├── setup_generator.rs# Windows-Remote-Agent-Generator
│   │       ├── web/              # HTTP-API (Axum)
│   │       │   ├── mod.rs        # Router-Setup, CORS
│   │       │   ├── api.rs        # REST-Endpoints
│   │       │   ├── sse.rs        # Server-Sent Events
│   │       │   └── ui.rs         # Eingebettetes HTML-UI
│   │       └── llm/              # LLM Gateway
│   │           ├── router.rs     # Multi-Provider Request-Routing
│   │           ├── api.rs        # LLM REST-Endpoints
│   │           ├── provider.rs   # LlmProvider Trait
│   │           ├── types.rs      # Request/Response-Types
│   │           ├── budget.rs     # Token-Budget & Cost-Tracking
│   │           └── providers/    # Ollama, Anthropic, Gemini
│   ├── egpu-manager-common/      # Shared Types & Traits
│   │   └── src/
│   │       ├── config.rs         # Config-Schema (836 LoC, 12 Sektionen)
│   │       ├── gpu.rs            # GpuStatus, GpuType, WarningLevel
│   │       ├── hal.rs            # Hardware-Abstraction-Layer Traits
│   │       └── error.rs          # Domänenspezifische Fehlertypen
│   ├── egpu-manager-detector/    # Projekt-Dependency-Scanner
│   │   └── src/lib.rs            # detect() → DetectionResult
│   ├── egpu-manager-cli/         # CLI-Tool
│   │   └── src/main.rs           # status, priority, config, remote, wizard, open
│   └── egpu-manager-gtk/         # GTK4 Desktop-Widget + Tray-Icon
│       └── src/
│           ├── main.rs           # GTK4 Main-Loop, Tray-Setup
│           ├── popup.rs          # 3-Tab Popup (GPU, Pipelines, LLM)
│           ├── tray.rs           # libappindicator Tray-Icon
│           ├── state.rs          # WidgetState
│           └── api_client.rs     # HTTP-Polling vom Daemon
├── clients/
│   ├── python/                   # Python-Client (egpu-llm-client)
│   ├── react/                    # React-Widget (EgpuPipelineWidget.tsx)
│   └── vue/                      # Vue-Widget (EgpuPipelineWidget.vue)
└── generated/                    # Generierte Installer/Scripts
```

## API-Endpunkte

**Lokale API** (127.0.0.1:7842, kein Auth):
- `GET /` — Eingebettetes Web-UI
- `GET /api/status` — Systemstatus
- `GET /api/pipelines` — Alle Pipelines
- `GET/PUT/POST /api/pipelines/{container}/*` — Pipeline-Management
- `POST /api/gpu/acquire|release` — GPU-Lease-Operationen
- `GET /api/gpu/recommend` — Platzierungsempfehlung
- `/api/llm/*` — LLM Gateway
- `GET /api/v1/discover` — Service-Discovery
- `GET /events/sse` — Server-Sent Events

**Remote API** (0.0.0.0:7843, Token-Auth):
- `POST /api/remote/register|unregister|heartbeat`

## Entwicklungsregeln

### Konventionen (aus dem Code abgeleitet)
- **Module/Dateien:** snake_case (`health_score.rs`, `link_health.rs`)
- **Structs/Enums/Traits:** PascalCase (`GpuStatus`, `AdmissionState`, `GpuMonitor`)
- **Funktionen/Methoden:** snake_case (`query_gpu_status`, `acquire_gpu_lease`)
- **Konstanten:** UPPER_CASE (`MAX_RETRIES`, `DEFAULT_TIMEOUT`)
- **Fehlerbehandlung:** `thiserror` für Domänenfehler, `anyhow::Result<T>` für Applikationslogik
- **Logging:** `tracing`-Makros (`debug!`, `info!`, `warn!`, `error!`)
- **Async:** Alle I/O-Operationen sind async (Tokio), Traits nutzen `#[async_trait]`
- **Shared State:** `Arc<Mutex<T>>` in async-Kontexten, `ArcSwap<Config>` für Hot-Reload
- **GPU-Identifikation:** Immer PCI-Bus-ID, nie nvidia-smi-Index (stabil über Reboots)
- **Config-Defaults:** Jedes Config-Feld hat einen `serde(default)` Fallback
- **Kommentare und Variablennamen:** Deutsch erlaubt (Projektkonvention)

### Architektur-Patterns
- **HAL (Hardware Abstraction Layer):** Traits in `hal.rs`, austauschbar für Tests/Mocks
- **State Machines:** Explizite Enums für Recovery, Warning, Admission
- **Config Hot-Reload:** `ArcSwap` erlaubt Konfigurationsänderungen ohne Neustart
- **Graceful Degradation:** Fallback auf interne GPU bei eGPU-Ausfall

## VERBOTENE OPERATIONEN

- **Keine destruktiven Datenbankoperationen** ohne explizite Bestätigung (SQLite unter `/var/lib/egpu-manager/`)
- **Keine Löschung von Dateien** außerhalb von `/tmp`
- **Kein Überschreiben von `.env`-Dateien** oder `llm-secrets.toml`
- **Keine git-Operationen** ohne Bestätigung (kein `force-push`, kein `reset --hard`, kein `checkout .`)
- **Keine Installation von System-Paketen** ohne Rückfrage
- **Kein Stoppen/Neustarten** von systemd-Services ohne Bestätigung
- **Kein Modifizieren** von `/etc/egpu-manager/config.toml` ohne explizite Anweisung
- **Keine nvidia-smi oder sysfs-Schreiboperationen** — nur Leseoperationen

## Pflichtverhalten

- Vor jeder Änderung an Produktionsdateien: kurze Zusammenfassung was geändert wird und warum
- Bei unklaren Anforderungen: nachfragen statt annehmen
- Die Spezifikation (`egpu-manager-spezifikation.md`) ist die kanonische Referenz für Architekturentscheidungen
- Config-Änderungen immer gegen das Schema in `common/src/config.rs` validieren
- Neue Endpoints müssen in `web/mod.rs` (Router) UND `web/api.rs` (Handler) eingetragen werden

## Testanweisungen

### Tests ausführen
```bash
# Alle Tests (Workspace)
cargo test

# Einzelner Crate
cargo test -p egpu-managerd
cargo test -p egpu-manager-common
cargo test -p egpu-manager-detector
cargo test -p egpu-manager-cli

# Einzelner Test
cargo test -p egpu-managerd test_name

# Mit Logging-Output
RUST_LOG=debug cargo test -- --nocapture
```

### Kompilierung prüfen
```bash
# Workspace kompilieren
cargo build

# Release-Build (für Deployment)
cargo build --release

# Nur Syntax/Typen prüfen (schneller)
cargo check
```

### Testabdeckung
- **131 Testfunktionen** verteilt über alle Crates
- Schwerpunkte: sysfs-Parsing, Scheduler-Logik, Recovery-State-Machine, Detector
- Async-Tests nutzen `#[tokio::test]`
- Mock-Unterstützung über HAL-Traits (Dependency Injection)

## Bekannte Fallstricke

### `#[allow(dead_code)]`-Annotationen
Mehrere Module enthalten `#[allow(dead_code)]`:
- `warning.rs` (6×) — Einige WarningStateMachine-Methoden noch nicht integriert
- `scheduler.rs` (5×) — Teile der Scheduler-Logik vorbereitet aber noch nicht aktiv
- `main.rs` (3×) — Module importiert aber teilweise noch nicht vollständig verbunden
- `remote_listener.rs`, `kmsg.rs`, `sse.rs` (je 1×)

Dies deutet darauf hin, dass einige Features implementiert aber noch nicht vollständig in den Hauptfluss integriert sind.

### Thermische Gradienten-Erkennung
- Kürzlich von 5s-Delta auf 60s-Sliding-Window (12 Samples) umgestellt
- Warnung erst ab ≥76°C (nicht nur bei >50% Auslastung) — Artefakt-Vermeidung
- TDP auf 300W angepasst für RTX 5070 Ti Boost-Headroom

### P-State P8 False-Positives
- Kürzlich behoben (Commit 4a0f2a3) — P8 ist im Idle-Zustand normal, kein Fehlerzustand

### Zwei separate HTTP-Listener
- Port 7842: Lokale API (127.0.0.1, kein Auth) — für CLI, Clients, Web-UI
- Port 7843: Remote API (0.0.0.0, Token-Auth) — für Remote-GPU-Registration
- Diese laufen als getrennte Tokio-Tasks, nicht als ein Server

### SQLite Bundled
- `rusqlite` kompiliert mit `bundled` Feature — bringt eigenes libsqlite3 mit
- Kein System-SQLite nötig, aber erhöht Build-Zeit

### GTK4 Build-Dependencies
- `egpu-manager-gtk` benötigt GTK4-Entwicklungspakete (`libgtk-4-dev`)
- `libappindicator` für Tray-Icon (Wayland/GNOME-kompatibel)

## Umgebungsvariablen

| Variable | Zweck |
|---|---|
| `RUST_LOG` | tracing-subscriber Filter (z.B. `debug`, `egpu_managerd=trace`) |
| `EGPU_MANAGER_URL` | Client-Override für Daemon-URL (Default: `http://127.0.0.1:7842`) |
| `EGPU_MANAGER_TOKEN` | Token für Remote-API-Authentifizierung |

## Deployment

```bash
# Release bauen und installieren
cargo build --release
sudo ./install-service.sh

# Oder manuell
sudo cp target/release/egpu-managerd /usr/local/bin/
sudo systemctl restart egpu-managerd

# Status prüfen
egpu-manager-cli status
# oder
curl http://127.0.0.1:7842/api/status
```
