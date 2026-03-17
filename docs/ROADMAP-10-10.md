# eGPU Manager Roadmap: 6.4 → 10/10

## Ausgangslage

| Dimension | Ist | Ziel |
|-----------|-----|------|
| Architektur | 8 | 10 |
| NVIDIA-Korrektheit | 6 | 10 |
| Fehlerbehandlung | 7 | 10 |
| Recovery-SM | 8 | 10 |
| Scheduler | 6 | 10 |
| Produktionsreife | 4 | 10 |
| Code-Qualität | 7 | 10 |
| **Gesamt** | **6.4** | **10** |

---

## Phase 1: VRAM-Accounting reparieren (KRITISCH)

**Ziel:** Scheduler arbeitet mit echten Werten statt Schätzungen.
**Aufwand:** ~3 Tage | **Blockiert:** Phase 3, 5, 7

### 1.1 Display-VRAM korrekt isolieren

**Problem:** `query_display_vram()` in `nvidia.rs:64-75` gibt `memory_used_mb` zurück — das enthält ALLE Prozesse (Display + Compute). Wenn ein Container auf der internen GPU läuft, wird dessen VRAM fälschlich als "Display-Reserve" gezählt.

**Lösung:**
```
Display-VRAM = memory_used_mb - sum(compute_process_vram)
```

**Änderungen:**
- `nvidia.rs` — Neue Methode `query_compute_processes(pci_address)`:
  ```bash
  nvidia-smi --query-compute-apps=pid,used_gpu_memory \
    --format=csv,noheader,nounits -i <gpu_index>
  ```
  Gibt `Vec<(u32, u64)>` (PID, VRAM_MB) zurück.
- `nvidia.rs` — `query_display_vram()` überarbeiten:
  ```rust
  let total_used = gpu.memory_used_mb;
  let compute_used: u64 = self.query_compute_processes(pci_address).await?
      .iter().map(|(_, vram)| vram).sum();
  Ok(total_used.saturating_sub(compute_used))
  ```
- `gpu.rs` — `ProcessVram` Struct existiert bereits (Zeile 109-113), nutzen.
- `hal.rs` — `query_process_vram` Trait-Methode existiert (Zeile 16), implementieren.

**Tests:**
- Unit-Test: Mock nvidia-smi Output mit 3 Compute-Prozessen, verifiziere Subtraktion
- Edge-Case: Keine Compute-Prozesse → Display-VRAM = total used
- Edge-Case: Compute > total (Race Condition) → saturating_sub → 0

### 1.2 Actual-VRAM im GPU-Poller aktualisieren

**Problem:** `update_actual_vram()` existiert in `scheduler.rs:234` aber wird nie aufgerufen. Jede Pipeline behält ihren Schätzwert für immer.

**Lösung:** Im GPU-Polling-Loop (`monitor.rs` ~Zeile 734) nach jedem nvidia-smi Poll:
1. `query_compute_processes()` für jede GPU aufrufen
2. PIDs den bekannten Pipelines zuordnen (via `/proc/<pid>/cgroup` → Container-Name)
3. `scheduler.update_actual_vram(pipeline_name, measured_vram)` aufrufen

**Vereinfachte Alternative** (falls PID-Zuordnung zu komplex):
- Pro GPU: `compute_vram_total = sum(compute_processes)`
- Verteile proportional auf bekannte Assignments nach Schätzung
- Besser als nichts, wenn PID-Mapping nicht zuverlässig ist

**Änderungen:**
- `nvidia.rs` — `query_compute_processes()` (siehe 1.1)
- `monitor.rs` — Im `gpu_polling_loop()` nach `st.gpu_status = gpus.clone()`:
  ```rust
  // Actual VRAM aus Compute-Prozessen aktualisieren
  if let Ok(procs) = gpu_monitor.query_compute_processes(&pci).await {
      let total_compute: u64 = procs.iter().map(|p| p.1).sum();
      // Update: Display-Reserve = total_used - compute_used + headroom
      let display_vram = gpu.memory_used_mb.saturating_sub(total_compute);
      let effective_reserve = (display_vram + 512).max(config.gpu.display_vram_reserve_mb);
      st.scheduler.update_display_reserve(target, effective_reserve);
  }
  ```

**Tests:**
- Bestehender Test `test_update_actual_vram` deckt Scheduler-Seite ab
- Neuer Test: Verify dass nach VRAM-Update die `vram_available()` korrekt steigt

### 1.3 CUDA-Context-Awareness vor Migration

**Problem:** `scheduler.migrate()` prüft nur VRAM-Kapazität. Migration mit aktivem CUDA-Context = Speicherkorruption.

**Lösung:**
- `nvidia.rs` — Neue Methode `query_active_cuda_pids(pci_address)`:
  ```bash
  nvidia-smi pmon -i <gpu_idx> -c 1 -s u
  ```
  Gibt PIDs mit aktivem CUDA-Kontext zurück.
- `recovery.rs` — Vor `execute_migration()`: Prüfe ob Container-PIDs in der aktiven CUDA-Liste sind.
  - Wenn ja: Erst Container graceful stoppen (Quiesce), dann migrieren.
  - Wenn nein: Direkt migrieren (sicher).

**Tests:**
- Mock `pmon` Output mit aktiven/inaktiven PIDs
- Test: Migration blockiert wenn CUDA aktiv

---

## Phase 2: Scheduler + Lease-Fixes (KRITISCH)

**Ziel:** Kein Phantom-VRAM, kein Ressourcen-Leak.
**Aufwand:** ~2 Tage | **Parallel zu Phase 1**

### 2.1 Xid-Error-Monitoring

**Problem:** kmsg.rs erkennt nur `CmpltTO` und `GPU progress error`. Xid-Errors (z.B. "Xid 79: GPU has fallen off the bus") werden ignoriert.

**Lösung:**
- `kmsg.rs` — Neuer Matcher `matches_xid_error(line, gpu_index)`:
  ```
  Pattern: "NVRM: Xid (PCI:XXXX:XX:XX.X): <id>,"
  ```
- `warning.rs` — Neuer Trigger `XidError { xid: u32 }` mit Severity-Mapping:
  | Xid | Bedeutung | Level |
  |-----|-----------|-------|
  | 13 | Graphics Engine Exception | Orange |
  | 31 | GPU memory page fault | Orange |
  | 43 | GPU stopped processing | Red |
  | 48 | Double Bit ECC Error | Red |
  | 61 | Internal firmware error | Orange |
  | 62 | Internal firmware error | Orange |
  | 69 | Graphics Engine Class Error | Orange |
  | 79 | GPU has fallen off the bus | Red |
  | 92 | High single-bit ECC error rate | Yellow |
  | andere | Unbekannt | Yellow |

- Optional: `nvidia.rs` — ECC-Counter Query (gibt `[N/A]` auf Consumer-GPUs, graceful handling):
  ```bash
  nvidia-smi --query-gpu=ecc.errors.corrected.volatile.total,ecc.errors.uncorrected.volatile.total
  ```

**Tests:**
- Unit-Tests für jedes Xid-Pattern
- Test: Xid 79 → WarningLevel::Red
- Test: Xid 92 → WarningLevel::Yellow

### 2.2 Lease-Heartbeat gegen Phantom-Leases

**Problem:** Crashed API-Client → Lease bleibt bis `expires_at` (default 3600s) → VRAM blockiert.

**Lösung A (Heartbeat):**
- `monitor.rs` — `GpuLease` erweitern um `last_heartbeat: DateTime<Utc>`
- `web/api.rs` — Neuer Endpoint `POST /api/gpu/heartbeat`:
  ```json
  { "lease_id": "..." }
  ```
  Aktualisiert `last_heartbeat`.
- `monitor.rs` — In `lease_expiry_loop()`: Auch Leases expiren wo `last_heartbeat` älter als `2 * heartbeat_interval` (default 60s).
- `web/mod.rs` — Route registrieren.

**Lösung B (einfacher, Renew-basiert):**
- Default Lease-Duration auf 300s reduzieren
- `POST /api/gpu/renew` Endpoint zum Verlängern
- Wer nicht renewed, verliert den Lease automatisch

**Config:**
- `lease_heartbeat_interval_seconds: u64` (default: 30)
- `lease_heartbeat_timeout_factor: u32` (default: 2)

**Tests:**
- Integration-Test: Lease erstellen, kein Heartbeat → Lease expired
- Integration-Test: Lease erstellen, Heartbeat → Lease bleibt

### 2.3 Preemption nach VRAM-Effizienz optimieren

**Problem:** `try_preempt()` in `scheduler.rs:567` sortiert nach Priorität, nicht nach VRAM. Verdrängt große Tasks zuerst.

**Lösung:** Zwei-Stufen-Sortierung:
```rust
// Erst nach Priorität (niedrigste zuerst verdrängen)
// Dann innerhalb gleicher Priorität nach VRAM aufsteigend
preemptable.sort_by(|a, b| {
    b.1.cmp(&a.1)                    // Priorität absteigend (höchste Zahl = niedrigste Prio zuerst)
        .then(a.2.cmp(&b.2))         // VRAM aufsteigend (kleinste zuerst)
});
```

**Tests:**
- Test: 2 Tasks gleicher Prio (2 GB und 8 GB), brauche 3 GB → verdränge 8 GB (ausreichend)
- Test: 2 Tasks gleicher Prio (2 GB und 8 GB), brauche 9 GB → verdränge beide

---

## Phase 3: Fehlerbehandlung härten (HOCH)

**Ziel:** Keine stillen Fehler, robuste Recovery.
**Aufwand:** ~3 Tage | **Benötigt Phase 1.3**

### 3.1 nvidia-smi Retry mit Exponential Backoff

**Lösung:** In `nvidia.rs` `run_nvidia_smi()`:
```rust
async fn run_nvidia_smi(&self, args: &[&str]) -> Result<String, GpuError> {
    let max_retries = 3;
    let base_delay = Duration::from_millis(200);

    for attempt in 0..max_retries {
        match self.run_nvidia_smi_once(args).await {
            Ok(output) => return Ok(output),
            Err(e) if attempt < max_retries - 1 => {
                let delay = base_delay * 2u32.pow(attempt);
                warn!("nvidia-smi Versuch {}/{} fehlgeschlagen: {e}, Retry in {:?}",
                    attempt + 1, max_retries, delay);
                tokio::time::sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}
```

**Config:**
- `nvidia_smi_max_retries: u32` (default: 3)
- `nvidia_smi_retry_base_delay_ms: u64` (default: 200)

### 3.2 Post-Recovery GPU-Validierung

**Problem:** Nach PCIe-Reset prüft Recovery nur ob nvidia-smi antwortet. GPU könnte trotzdem defekt sein.

**Lösung:** Nach `check_nvidia_smi_available()` in `recovery.rs`:
1. `query_all()` → Prüfe `memory_total_mb` == erwarteter Wert
2. CUDA-Watchdog mit `--validate` Flag ausführen (kleine MatMul, Ergebnis prüfen)
3. Wenn Validierung fehlschlägt → `StageResult::Failed`

**Neue Methode in nvidia.rs:**
```rust
pub async fn validate_gpu_functional(&self, pci_address: &str, expected_vram_mb: u64) -> Result<bool, GpuError> {
    let gpus = self.query_all().await?;
    let gpu = gpus.iter().find(|g| normalize_pci_address(&g.pci_address) == normalize_pci_address(pci_address))
        .ok_or(GpuError::GpuNotFound { pci_address: pci_address.to_string() })?;

    // VRAM-Plausibilitätscheck (Toleranz 5%)
    let tolerance = expected_vram_mb / 20;
    if gpu.memory_total_mb < expected_vram_mb.saturating_sub(tolerance) {
        return Ok(false);
    }

    // Temperatur-Plausibilitätscheck (0°C oder > 110°C = Sensor-Fehler)
    if gpu.temperature_c == 0 || gpu.temperature_c > 110 {
        return Ok(false);
    }

    Ok(true)
}
```

### 3.3 Quiesce-Hook Erfolgsrate tracken

**Lösung:** In `execute_quiesce()` in `recovery.rs`:
```rust
let mut success = 0;
let mut failed = 0;
for hook in &hooks {
    match run_hook(hook).await {
        Ok(_) => success += 1,
        Err(e) => { warn!("Hook {} fehlgeschlagen: {e}", hook.container); failed += 1; }
    }
}
let total = success + failed;
if total > 0 && failed > total / 2 {
    return StageResult::Failed(format!("Quiesce: {failed}/{total} Hooks fehlgeschlagen"));
}
```

### 3.4 Thunderbolt Pre-Flight-Check

**Lösung:** In `monitor.rs` `select_lease_placement()` vor eGPU-Zuweisung:
```rust
fn thunderbolt_link_healthy(state: &MonitorState, egpu_pci: &str) -> bool {
    // Prüfe GPU-Status
    if let Some(gpu) = state.gpu_status.iter().find(|g| g.pci_address == egpu_pci) {
        if gpu.online_status != GpuOnlineStatus::Online { return false; }
        if gpu.temperature_c == 0 { return false; }  // Sensor nicht lesbar = Link-Problem
    } else {
        return false;  // GPU nicht in nvidia-smi gefunden
    }

    // Prüfe Health Score
    if state.health_score.current_score() < state.health_score.warning_threshold() {
        return false;
    }

    true
}
```

---

## Phase 4: Produktionsreife (MITTEL)

**Ziel:** Observability, Sicherheit, Stabilität.
**Aufwand:** ~3 Tage | **Parallel zu Phase 1-3**

### 4.1 Prometheus-Metriken

**Dependency:** `metrics = "0.24"` + `metrics-exporter-prometheus = "0.16"`

**Neues Modul:** `crates/egpu-managerd/src/metrics.rs`

**Metriken:**
```
# GPU-Telemetrie
egpu_gpu_temperature_celsius{gpu="egpu|internal"}
egpu_gpu_utilization_percent{gpu="egpu|internal"}
egpu_gpu_vram_used_mb{gpu="egpu|internal"}
egpu_gpu_vram_total_mb{gpu="egpu|internal"}
egpu_gpu_vram_available_mb{gpu="egpu|internal"}
egpu_gpu_power_draw_watts{gpu="egpu|internal"}
egpu_gpu_pstate{gpu="egpu|internal"}

# Scheduler
egpu_scheduler_assignments_total{gpu="egpu|internal"}
egpu_scheduler_queue_length
egpu_scheduler_vram_used_mb{gpu="egpu|internal"}
egpu_scheduler_display_reserve_mb

# Daemon
egpu_warning_level                    # Gauge 0-3 (Green-Red)
egpu_health_score                     # Gauge 0-100
egpu_active_leases_total
egpu_recovery_active                  # 0 oder 1
egpu_nvidia_smi_response_ms           # Histogram
egpu_nvidia_smi_timeouts_total        # Counter
egpu_aer_errors_total                 # Counter

# Thunderbolt
egpu_pcie_link_speed_gts
egpu_pcie_link_width
egpu_pcie_throughput_tx_kbps
egpu_pcie_throughput_rx_kbps
```

**Endpoint:** `GET /metrics` (Prometheus text format)

**Änderungen:**
- `Cargo.toml` — Dependencies hinzufügen
- Neues `metrics.rs` — Registry + Helper-Funktionen
- `web/mod.rs` — `/metrics` Route
- `monitor.rs` — Metriken im Polling-Loop aktualisieren

### 4.2 GPU-Treiberversions-Check

**Lösung:** Beim Start in `main.rs`:
```bash
nvidia-smi --query-gpu=driver_version --format=csv,noheader
```
- Log: `info!("NVIDIA-Treiber: {}", version)`
- Warn wenn < 550 (RTX 5000 Serie benötigt 550+)
- Warn wenn verschiedene GPUs verschiedene Versionen haben (sollte nicht passieren, aber...)

### 4.3 API Rate-Limiting

**Dependency:** `tower = { version = "0.5", features = ["limit"] }` oder `governor = "0.7"`

**Lösung:** Middleware auf mutierenden Routes:
- `/api/gpu/acquire`, `/api/gpu/release`, `/api/gpu/heartbeat` → 60 req/min
- `/api/pipelines/*/priority`, `/api/pipelines/*/assign` → 30 req/min
- `/api/status`, `/api/pipelines`, `/metrics`, `/events/sse` → unlimited

**Config:** `api_rate_limit_rpm: u32` in `LocalApiConfig` (default: 60)

### 4.4 Startup-Validierung

**Lösung:** In `main.rs` nach `gpu_monitor.query_all()`:
```rust
if gpus.is_empty() {
    error!("Keine GPUs gefunden — Daemon kann nicht starten");
    std::process::exit(1);
}

let has_internal = gpus.iter().any(|g| g.pci_address == config.gpu.internal_pci_address);
let has_egpu = gpus.iter().any(|g| g.pci_address == config.gpu.egpu_pci_address);

if !has_internal {
    error!("Interne GPU {} nicht gefunden — Daemon kann nicht starten",
        config.gpu.internal_pci_address);
    std::process::exit(1);
}

if !has_egpu {
    warn!("eGPU {} nicht gefunden — starte im Internal-Only-Modus",
        config.gpu.egpu_pci_address);
    // scheduler.set_egpu_available(false) wird im Orchestrator gesetzt
}
```

---

## Phase 5: System-Verbesserungen (MITTEL)

**Ziel:** NUMA, Power, Atomares Scheduling, PCI-Validierung.
**Aufwand:** ~3 Tage | **Benötigt Phase 1**

### 5.1 NUMA-Awareness

- `gpu.rs` — `GpuStatus` erweitern: `pub numa_node: Option<i32>`
- `nvidia.rs` — NUMA-Node lesen: `/sys/bus/pci/devices/<pci>/numa_node`
- `monitor.rs` — Bei Lease-Placement: Bevorzuge GPU auf gleichem NUMA-Node
- Niedrige Auswirkung auf Single-Socket Laptop, wichtig für Korrektheit

### 5.2 Power-Budget-Überwachung

**Config:**
```toml
[gpu]
max_combined_power_w = 450   # PSU-Limit minus System-Grundlast
power_warning_percent = 90
```

**Lösung:** Im Polling-Loop:
```rust
let combined_power: f64 = gpus.iter().map(|g| g.power_draw_w).sum();
if combined_power > max_combined * 0.9 {
    warn!("Kombinierte GPU-Last {:.0}W > 90% von {:.0}W Budget", combined_power, max_combined);
    // Neue eGPU-Tasks blockieren (Admission → Drain)
}
```

### 5.3 Atomares Multi-GPU-Scheduling

**Lösung:** Neue Methode in `scheduler.rs`:
```rust
pub fn schedule_multi_gpu(&mut self, requests: Vec<ScheduleRequest>) -> Result<Vec<ScheduleResult>, String> {
    // Snapshot des aktuellen Zustands
    let snapshot_assignments = self.assignments.clone();
    let snapshot_leases = self.lease_reservations.clone();

    let mut results = Vec::new();
    for req in &requests {
        let result = self.schedule(req.clone());
        if matches!(result, ScheduleResult::Queued) {
            // Rollback: Alles rückgängig machen
            self.assignments = snapshot_assignments;
            self.lease_reservations = snapshot_leases;
            return Err(format!("Multi-GPU Scheduling fehlgeschlagen für {}", req.name));
        }
        results.push(result);
    }
    Ok(results)
}
```

### 5.4 PCI-Adress-Validierung mit Regex

**Lösung:** In `config.rs` `validate_pci_address()`:
```rust
fn validate_pci_address(addr: &str) -> anyhow::Result<()> {
    // Format: DDDD:BB:DD.F (4-digit domain, 2-digit bus, 2-digit device, 1-digit function)
    let re = regex::Regex::new(r"^[0-9a-fA-F]{4}:[0-9a-fA-F]{2}:[0-9a-fA-F]{2}\.[0-7]$").unwrap();
    if !re.is_match(addr) {
        anyhow::bail!("Ungültige PCI-Adresse: {addr} (erwartet Format DDDD:BB:DD.F, z.B. 0000:05:00.0)");
    }
    Ok(())
}
```

**Dependency:** `regex` Crate (oder `lazy_static` für Compile-einmalig)

---

## Phase 6: Code-Qualität (NIEDRIG)

**Ziel:** Kein Tech-Debt, sauberer Code.
**Aufwand:** ~1.5 Tage | **Nach Phase 1-5**

### 6.1 Dead-Code Cleanup

Nach Phase 1-5 überprüfen: Welche `#[allow(dead_code)]` sind jetzt überflüssig?

**Aktuell 17 Instanzen:**
| Datei | Methode/Modul | Aktion nach Roadmap |
|-------|---------------|---------------------|
| `main.rs:2` | `mod db` | Wird benutzt → `allow` entfernen |
| `main.rs:11` | `mod ollama` | Wird benutzt → `allow` entfernen |
| `main.rs:13` | `mod recovery` | Wird in Phase 3 aktiv → `allow` entfernen |
| `scheduler.rs:73` | `ScheduleResult` | Variants alle benutzt → `allow` entfernen |
| `scheduler.rs:146` | `set_egpu_available` | Phase 4.4 nutzt es → `allow` entfernen |
| `scheduler.rs:342` | `remove()` | Phase 2.2 nutzt es → `allow` entfernen |
| `scheduler.rs:423` | `update_workload()` | Phase 1.2 nutzt es → `allow` entfernen |
| `scheduler.rs:642` | `try_dequeue()` | `remove()` ruft es auf → `allow` entfernen |
| `warning.rs` (6x) | Diverse | Prüfen ob Phase 3 sie aktiviert |
| `remote_listener.rs:30` | Feld | Prüfen |
| `kmsg.rs:29` | Feld | Prüfen |

### 6.2 Structured Audit Logging

- `main.rs` — JSON-Tracing-Layer konfigurieren:
  ```rust
  tracing_subscriber::fmt()
      .json()
      .with_env_filter(EnvFilter::from_default_env())
      .init();
  ```
- Schlüssel-Funktionen mit `#[instrument]` annotieren:
  - `schedule()`, `migrate()`, `reserve_lease()`, `release_lease()`
  - `execute_recovery_stage()`
  - `gpu_acquire()`, `gpu_release()`

### 6.3 nvidia-smi Output-Caching

**Lösung:** Wrapper-Struct:
```rust
pub struct CachedGpuMonitor {
    inner: NvidiaSmiMonitor,
    cache: Arc<Mutex<Option<(Vec<GpuStatus>, Instant)>>>,
    ttl: Duration,
}

impl CachedGpuMonitor {
    pub async fn query_all(&self) -> Result<Vec<GpuStatus>, GpuError> {
        let mut cache = self.cache.lock().await;
        if let Some((ref data, ref ts)) = *cache {
            if ts.elapsed() < self.ttl {
                return Ok(data.clone());
            }
        }
        let result = self.inner.query_all().await?;
        *cache = Some((result.clone(), Instant::now()));
        Ok(result)
    }
}
```

**TTL:** 1s (verhindert Mehrfach-Abfragen im gleichen Poll-Zyklus)

---

## Phase 7: Architektur-Erweiterungen (FORTGESCHRITTEN)

**Ziel:** Best-in-Class GPU-Management.
**Aufwand:** ~4 Tage | **Benötigt Phase 1-3**

### 7.1 Erweitertes GPU-Health-Modell

**Neue `HealthEventKind` Varianten:**
```rust
pub enum HealthEventKind {
    AerError,           // bestehend
    PcieTransient,      // bestehend
    NvidiaSmiSlow,      // bestehend
    TemperatureSpike,   // bestehend
    PstateAnomaly,      // bestehend
    // NEU:
    SmClockVariance,    // GPU-Takt-Abweichung > 20% vom Baseline
    PowerInstability,   // Power-Draw-Varianz > 30% innerhalb 10s
    EccError,           // ECC-Fehler (falls verfügbar)
    XidError,           // NVIDIA Xid Error (aus kmsg)
    VramFragmentation,  // Allokation fehlschlägt trotz freiem VRAM
}
```

**SM-Clock-Tracking:** Im Polling-Loop:
```rust
// Baseline: gleitender Durchschnitt über 60s
// Wenn aktueller clock_graphics_mhz < 80% von Baseline UND utilization > 50%
// → HealthEventKind::SmClockVariance
```

**Power-Stabilität:** Varianzberechnung über 10s-Fenster (2 Polls):
```rust
// Wenn Standardabweichung > 30% des Mittelwerts
// → HealthEventKind::PowerInstability
```

### 7.2 CUDA-Stress-Test nach Recovery

**Lösung:** Nach jeder erfolgreichen Recovery-Stage (1, 3):
1. Führe CUDA-Watchdog mit `--stress` Flag aus
2. Test: 256 MB VRAM allokieren, MatMul, Ergebnis verifizieren
3. Timeout: 10s
4. Bei Fehler → `StageResult::Failed` → nächste Recovery-Stufe

**Watchdog-Erweiterung** (externes Binary, nicht im Rust-Code):
```bash
egpu-watchdog --stress --gpu-index 1 --timeout 10
# Exit 0 = OK, Exit 1 = GPU defekt
```

### 7.3 Daemon-Failover (Design-Dokument)

Für eine produktionsreife Failover-Lösung wäre nötig:
- Leader Election via SQLite (oder etcd/consul)
- Shared State über die bestehende SQLite-DB
- Watchdog-Prozess der den Daemon überwacht

**Entscheidung:** Nicht implementieren — zu komplex für Single-Machine-Setup. Stattdessen: systemd `Restart=on-failure` mit `RestartSec=5s` ist ausreichend.

---

## Abhängigkeitsgraph

```
Phase 1 (VRAM) ─────────┬──→ Phase 3 (Error Handling)
                         │
Phase 2 (Scheduler)      ├──→ Phase 5 (System)
                         │
                         └──→ Phase 7 (Architektur)

Phase 4 (Production) ── unabhängig, parallel zu allem

Phase 6 (Code Quality) ── nach Phase 1-5
```

## Parallelisierungsmatrix

```
Woche 1:  [Phase 1: 1.1+1.2] + [Phase 2: 2.1+2.2+2.3] + [Phase 4: 4.1]
Woche 2:  [Phase 1: 1.3]      + [Phase 3: 3.1+3.2]      + [Phase 4: 4.2+4.3+4.4]
Woche 3:  [Phase 3: 3.3+3.4]  + [Phase 5: 5.1+5.2+5.3+5.4]
Woche 4:  [Phase 6: komplett]  + [Phase 7: 7.1+7.2]
```

## Erwartete Scorecard nach Implementierung

| Dimension | Vorher | Phase 1-2 | Phase 3-4 | Phase 5-6 | Phase 7 |
|-----------|--------|-----------|-----------|-----------|---------|
| Architektur | 8 | 8.5 | 9 | 9.5 | **10** |
| NVIDIA-Korrektheit | 6 | 8.5 | 9 | 9.5 | **10** |
| Fehlerbehandlung | 7 | 7.5 | 9 | 9.5 | **10** |
| Recovery-SM | 8 | 8 | 9.5 | 9.5 | **10** |
| Scheduler | 6 | 9 | 9 | 9.5 | **10** |
| Produktionsreife | 4 | 5 | 8 | 9 | **10** |
| Code-Qualität | 7 | 7.5 | 8 | 9.5 | **10** |
| **Gesamt** | **6.4** | **7.7** | **8.8** | **9.4** | **10** |

---

## Betroffene Dateien (Zusammenfassung)

| Datei | Phasen | Änderungsumfang |
|-------|--------|-----------------|
| `nvidia.rs` | 1,2,3,4,6 | Groß: compute-apps, retry, validation, caching, driver check |
| `monitor.rs` | 1,2,3,4,5 | Groß: VRAM-Updates, heartbeat, TB-check, metrics, power |
| `scheduler.rs` | 1,2,5 | Mittel: preemption, multi-GPU, dead-code |
| `recovery.rs` | 1,3,7 | Mittel: CUDA-check, validation, quiesce-tracking, stress-test |
| `kmsg.rs` | 2 | Klein: Xid-Patterns |
| `warning.rs` | 2 | Klein: XidError-Trigger |
| `health_score.rs` | 7 | Mittel: Neue Event-Typen |
| `config.rs` | 3,4,5 | Klein: Neue Config-Felder |
| `web/api.rs` | 2,4 | Mittel: Heartbeat, Metrics |
| `web/mod.rs` | 2,4 | Klein: Neue Routes |
| `main.rs` | 4,6 | Klein: Startup-Validierung, Logging |
| `gpu.rs` | 5 | Klein: NUMA-Feld |
| **Neu: `metrics.rs`** | 4 | Mittel: Prometheus-Registry |
