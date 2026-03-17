# Subagenten für egpu-manager

## Übersicht

Dieses Dokument definiert spezialisierte Subagenten für die Arbeit am egpu-manager Projekt. Jeder Agent hat einen klar abgegrenzten Verantwortungsbereich, um Konflikte bei paralleler Arbeit zu vermeiden.

---

## 1. Test-Agent

**Zweck:** Tests ausführen, Testergebnisse analysieren, fehlende Tests identifizieren.

**Aufruf:**
```
Agent(subagent_type="general-purpose", prompt="Führe cargo test im egpu-Workspace aus und analysiere die Ergebnisse. Melde Failures mit vollem Kontext.")
```

**Erlaubte Tools:** `Bash` (nur `cargo test`, `cargo check`), `Read`, `Grep`, `Glob`

**Dateibereiche:**
- Lesezugriff: gesamtes `crates/` Verzeichnis
- Kein Schreibzugriff — meldet Ergebnisse nur zurück

**Typische Aufgaben:**
- `cargo test` nach Code-Änderungen ausführen
- `cargo check` für schnelle Kompilierungsprüfung
- Fehlgeschlagene Tests mit Kontext aus dem Quellcode erklären
- Testabdeckung für neue Features prüfen

---

## 2. Daemon-Core Agent

**Zweck:** Änderungen am Core-Daemon (Monitoring, Scheduling, Recovery, Hardware-Interaktion).

**Aufruf:**
```
Agent(subagent_type="general-purpose", prompt="Arbeite am egpu-managerd Daemon-Core: [Aufgabe]")
```

**Erlaubte Tools:** `Read`, `Edit`, `Write`, `Grep`, `Glob`, `Bash` (nur `cargo check -p egpu-managerd`, `cargo test -p egpu-managerd`)

**Dateibereiche (exklusiv):**
- `crates/egpu-managerd/src/monitor.rs`
- `crates/egpu-managerd/src/scheduler.rs`
- `crates/egpu-managerd/src/recovery.rs`
- `crates/egpu-managerd/src/warning.rs`
- `crates/egpu-managerd/src/health_score.rs`
- `crates/egpu-managerd/src/nvidia.rs`
- `crates/egpu-managerd/src/docker.rs`
- `crates/egpu-managerd/src/ollama.rs`
- `crates/egpu-managerd/src/aer.rs`
- `crates/egpu-managerd/src/link_health.rs`
- `crates/egpu-managerd/src/sysfs.rs`
- `crates/egpu-managerd/src/kmsg.rs`
- `crates/egpu-managerd/src/db.rs`
- `crates/egpu-managerd/src/main.rs`

**Nicht zuständig für:** Web-API, LLM-Gateway, GTK-Widget, CLI

---

## 3. Web-API & LLM-Gateway Agent

**Zweck:** HTTP-Endpoints, SSE, LLM-Provider-Integration, Router-Logik.

**Aufruf:**
```
Agent(subagent_type="general-purpose", prompt="Arbeite an der Web-API oder dem LLM-Gateway: [Aufgabe]")
```

**Erlaubte Tools:** `Read`, `Edit`, `Write`, `Grep`, `Glob`, `Bash` (nur `cargo check -p egpu-managerd`, `cargo test -p egpu-managerd`)

**Dateibereiche (exklusiv):**
- `crates/egpu-managerd/src/web/mod.rs`
- `crates/egpu-managerd/src/web/api.rs`
- `crates/egpu-managerd/src/web/sse.rs`
- `crates/egpu-managerd/src/web/ui.rs`
- `crates/egpu-managerd/src/remote_listener.rs`
- `crates/egpu-managerd/src/llm/*.rs` (alle Dateien im LLM-Submodul)

**Shared (Lesen erlaubt, Schreiben nur in Absprache):**
- `crates/egpu-manager-common/src/config.rs` (Config-Schema betrifft beide Agents)

---

## 4. Common-Types Agent

**Zweck:** Änderungen an gemeinsamen Typen, Traits, Config-Schema, Fehlertypen.

**Aufruf:**
```
Agent(subagent_type="general-purpose", prompt="Arbeite am egpu-manager-common Crate: [Aufgabe]")
```

**Erlaubte Tools:** `Read`, `Edit`, `Write`, `Grep`, `Glob`, `Bash` (nur `cargo check`, `cargo test -p egpu-manager-common`)

**Dateibereiche (exklusiv):**
- `crates/egpu-manager-common/src/*.rs`

**Achtung:** Änderungen hier haben Auswirkungen auf alle anderen Crates. Immer `cargo check` für den gesamten Workspace ausführen.

---

## 5. Client-Libraries Agent

**Zweck:** Python-Client, React-Widget, Vue-Widget.

**Aufruf:**
```
Agent(subagent_type="general-purpose", prompt="Arbeite an den Client-Libraries: [Aufgabe]")
```

**Erlaubte Tools:** `Read`, `Edit`, `Write`, `Grep`, `Glob`

**Dateibereiche (exklusiv):**
- `clients/python/` — Python-Package (egpu-llm-client)
- `clients/react/` — React TSX-Widget
- `clients/vue/` — Vue SFC-Widget

**Keine Bash-Ausführung** außer für `pip install -e clients/python/` oder ähnliche Paket-Tests.

---

## 6. Explore/Research Agent

**Zweck:** Codebase-Analyse, Architektur-Fragen beantworten, Abhängigkeiten nachverfolgen.

**Aufruf:**
```
Agent(subagent_type="Explore", prompt="[Frage zur Codebase]")
```

**Erlaubte Tools:** `Read`, `Grep`, `Glob` — kein Schreibzugriff

**Dateibereiche:** Gesamtes Repository (nur lesend)

**Typische Aufgaben:**
- "Welche Module nutzen den GpuStatus-Typ?"
- "Wie fließt ein `/api/gpu/acquire` Request durch den Code?"
- "Welche Config-Felder werden im Scheduler verwendet?"

---

## 7. Deployment & Config Agent

**Zweck:** Deployment-Scripts, systemd-Konfiguration, config.toml-Änderungen.

**Aufruf:**
```
Agent(subagent_type="general-purpose", prompt="Arbeite an Deployment/Config: [Aufgabe]")
```

**Erlaubte Tools:** `Read`, `Edit`, `Write`, `Grep`, `Glob`

**Dateibereiche (exklusiv):**
- `deploy.sh`
- `install-service.sh`
- `egpu-managerd.service`
- `generated/*.sh`
- `config.toml` (nur nach expliziter Bestätigung)
- `pipeline-profiles.toml`

**Kein Bash** — Deployment-Scripts sollen nur bearbeitet, nicht ausgeführt werden.

---

## Konflikt-Vermeidung

### Shared Files (kritisch)
Diese Dateien werden von mehreren Agents gelesen und dürfen nur von einem gleichzeitig bearbeitet werden:

| Datei | Lese-Agents | Schreib-Agent |
|---|---|---|
| `common/src/config.rs` | Daemon-Core, Web-API | Common-Types |
| `common/src/gpu.rs` | Daemon-Core, Web-API, Clients | Common-Types |
| `common/src/hal.rs` | Daemon-Core | Common-Types |
| `common/src/error.rs` | Alle | Common-Types |
| `managerd/src/main.rs` | Web-API | Daemon-Core |

### Parallelisierung
Folgende Agents können sicher parallel laufen:
- **Test-Agent** + jeder andere (nur lesend)
- **Explore-Agent** + jeder andere (nur lesend)
- **Client-Libraries** + **Daemon-Core** (keine Überschneidung)
- **Client-Libraries** + **Web-API** (keine Überschneidung)
- **Deployment** + **Daemon-Core** (keine Überschneidung)

Folgende Kombinationen erfordern Sequenzierung:
- **Common-Types** → dann **Daemon-Core** oder **Web-API** (wegen Typ-Abhängigkeiten)
- **Daemon-Core** + **Web-API** gleichzeitig nur wenn verschiedene Dateien bearbeitet werden

### Workflow-Empfehlung
1. **Analyse:** Explore-Agent für Kontext
2. **Implementierung:** Zuständiger Agent (Core, Web-API, Common, Clients)
3. **Verifikation:** Test-Agent nach jeder Änderung
4. **Review:** Explore-Agent für Auswirkungsanalyse
