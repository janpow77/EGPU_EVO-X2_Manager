# Bug: Chat und Embeddings über Gateway funktionieren nicht mehr

## Aktueller Zustand (2026-03-29 14:10)

### Was funktioniert
- `GET /api/status` — OK (Daemon läuft, Uptime 1700s, Grün)
- `GET /api/llm/providers` — OK (zeigt alle Provider als healthy)

### Was NICHT funktioniert

#### 1. Chat-Endpoint: 503 "server busy"
```bash
curl -s -X POST http://localhost:7842/api/llm/chat/completions \
  -H "X-App-Id: audit_designer" -H "Content-Type: application/json" \
  -d '{"model":"qwen2.5:72b-instruct-q4_K_M",
       "messages":[{"role":"user","content":"Sag nur OK"}],
       "max_tokens":10,"stream":false}'
```
**Response:**
```json
{"error":{"message":"API error: 503 {\"error\":{\"message\":\"server busy, please try again. maximum pending requests exceeded\",...}}","type":"provider_error"}}
```
Das passiert SOFORT (0.08s), nicht nach Timeout — die EVO X2 lehnt den Request direkt ab.

#### 2. Embedding-Endpoint: Timeout
```bash
curl -s --max-time 10 -X POST http://localhost:7842/api/llm/embeddings \
  -H "X-App-Id: audit_designer" -H "Content-Type: application/json" \
  -d '{"model":"bge-m3","input":["test"]}'
```
**Response:** Leere Antwort nach 10s Timeout (vorher funktionierte das in 0.1s)

#### 3. Direkter Ollama-Call an EVO X2: Leere Antworten
```bash
curl -s http://100.81.4.99:11434/api/chat \
  -d '{"model":"qwen2.5:72b-instruct-q4_K_M",
       "messages":[{"role":"user","content":"OK?"}],
       "stream":false,"options":{"num_predict":10}}'
```
**Response:** `{"message":{"content":""},...}` — Antwort kommt sofort aber content ist leer.

## Diagnose

### Ollama auf der EVO X2 ist überlastet

Die Fehlermeldung `maximum pending requests exceeded` bedeutet: Ollama hat zu viele gleichzeitige Requests in der Queue. Das kann passieren wenn:

1. **Vorherige Requests noch laufen** — z.B. hängengebliebene Chat-Requests ohne max_tokens die noch generieren
2. **Embedding-Batch-Job** — der audit_designer Embedding-Job hat vorher 37.000 Chunks mit batch_size=100 über die EVO X2 geschickt. Möglicherweise sind noch Requests in der Queue.
3. **Ollama num_parallel** zu niedrig — default ist 1 für große Modelle. Wenn ein Request läuft, werden alle weiteren mit 503 abgelehnt.

### Warum auch leere Antworten direkt?

Der direkte Ollama-Call gibt `content: ""` zurück. Das deutet darauf hin, dass Ollama den Request annimmt aber sofort eine leere Antwort zurückgibt. Mögliche Ursachen:
- VRAM-Problem (Modell teilweise ausgelagert)
- Ollama-Bug bei gleichzeitigem Embed + Chat
- Modell korrupt im Cache

## Empfohlene Fixes

### Sofort: Ollama auf EVO X2 neustarten
```bash
# SSH zur EVO X2 oder via Tailscale
ssh 100.81.4.99 "sudo systemctl restart ollama"
# Oder: curl -X POST http://100.81.4.99:11434/api/unload -d '{"model":"qwen2.5:72b-instruct-q4_K_M"}'
```

### Gateway: Pending-Request-Queue erhöhen oder Retry
In der Gateway-Konfiguration oder im Router: Wenn Ollama 503 zurückgibt, kurz warten (5s) und 1x retrien statt sofort Fehler zurückzugeben.

### Gateway: Request-Queue-Management
- Tracke laufende Requests pro Provider
- Wenn ein Provider überlastet ist, route zum nächsten (Fallback)
- Logge Queue-Depth damit man Überlast erkennt

### Langfristig: num_parallel auf EVO X2
In der Ollama-Config auf der EVO X2:
```
OLLAMA_NUM_PARALLEL=2
```
Das erlaubt 2 gleichzeitige Requests (bei 128 GB unified RAM sollte das gehen).

## Chronologie

1. **09:14** — Embedding-Job (37k Chunks) über Gateway gestartet, funktionierte mit ~50 Chunks/s
2. **~10:00** — Embedding-Job teilweise fertig, einige Batches scheitern
3. **~11:00** — Chat-Requests fangen an zu hängen (kein max_tokens)
4. **12:40** — eGPU Manager Daemon neu gestartet
5. **13:00** — Chat-Endpoint gibt 503 "server busy" zurück
6. **13:20** — Auch Embeddings funktionieren nicht mehr (Timeout)
7. **14:10** — Ollama auf EVO X2 gibt leere Antworten auf direkte Calls

## Betroffene Consumer

- audit_designer: KB Text Generator (Kompendium-Generierung blockiert, läuft über Anthropic-Fallback)
- audit_designer: RAG-Suche Query-Embedding (fällt auf lokale CPU zurück)
- Alle Projekte die den Gateway für LLM nutzen
