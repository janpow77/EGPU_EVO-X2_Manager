# Feature: Streaming als Default für Chat-Requests

## Problem

Non-streaming Chat-Requests (`stream: false`) an die EVO X2 führen regelmäßig zu Timeouts. Qwen3:32b generiert bei langen Prompts (RAG-Kontext mit 5000+ Input-Tokens) 3-7 Minuten lang Tokens. Ollama puffert die gesamte Antwort und liefert sie erst am Ende aus. Das übersteigt den Gateway-Timeout und der Client bekommt eine leere Antwort oder 502.

Streaming (`stream: true`) umgeht das Problem vollständig — Tokens werden sofort einzeln ausgeliefert, der erste Token kommt nach wenigen Sekunden, und der Client baut die Antwort inkrementell zusammen.

## Betroffene Consumer

- **audit_designer**: `llm_gateway_client.py` → KB Text Generator, RAG Ask, Checklisten-KI, Notebook-RAG
- **auditworkshop**: `ollama_service.py` (bereits auf Streaming umgestellt)
- Jeder Client der `POST /api/llm/chat/completions` mit `stream: false` nutzt

## Gewünschte Änderung

### 1. `_call_openai_compatible()` auf Streaming umstellen

**Datei:** `backend/app/modules/vp_ai/services/llm_gateway_client.py`

Diese Funktion wird von `_call_egpu_gateway()` (EVO X2 direkt) und als generischer OpenAI-kompatibler Client genutzt. Sie ist der zentrale Punkt für die Umstellung.

**Signatur bleibt gleich** — `call_llm()` gibt weiterhin `str` zurück. Die Streaming-Logik ist für alle Aufrufer transparent.

```python
def _call_openai_compatible(
    base_url: str,
    model: str,
    system_prompt: str,
    user_prompt: str,
    temperature: float,
    max_tokens: int,
    label: str,
) -> str | None:
    url = f"{base_url}/v1/chat/completions"
    payload = {
        "model": model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt},
        ],
        "temperature": temperature,
        "max_tokens": max_tokens,
        "stream": True,
    }

    try:
        start = time.monotonic()
        content_parts: list[str] = []
        usage_data: dict = {}

        # Timeouts: 30s Verbindungsaufbau, 120s zwischen SSE-Events
        timeout = httpx.Timeout(connect=30.0, read=120.0, write=30.0, pool=30.0)

        with httpx.stream(
            "POST", url, json=payload, timeout=timeout,
            headers={"X-App-Id": "audit_designer"},
        ) as response:
            # HTTP-Fehler VOR dem Iterieren prüfen — sonst werden 502/503 verschluckt
            if response.status_code != 200:
                body = response.read().decode("utf-8", errors="replace")
                logger.warning("%s HTTP %s: %s", label, response.status_code, body[:200])
                return None

            for line in response.iter_lines():
                # SSE-Format: "data: {json}" oder "data: [DONE]"
                if not line.startswith("data: "):
                    continue

                data_str = line[6:]  # Strip "data: " prefix

                # [DONE] ist KEIN JSON — separat abfangen
                if data_str.strip() == "[DONE]":
                    break

                try:
                    chunk = json.loads(data_str)
                except json.JSONDecodeError:
                    logger.debug("%s: Malformed SSE chunk: %s", label, data_str[:100])
                    continue

                choices = chunk.get("choices", [])
                if not choices:
                    continue

                delta = choices[0].get("delta", {})

                # Nur "content" sammeln — NICHT "reasoning_content"
                # Qwen3 sendet Think-Blöcke in "reasoning_content" und die
                # eigentliche Antwort in "content". Leere Deltas (Heartbeats)
                # kommen als delta={} und werden hier korrekt übersprungen.
                content = delta.get("content")
                if content:
                    content_parts.append(content)

                # Finish-Reason: Letzter Chunk enthält usage-Daten
                if choices[0].get("finish_reason") is not None:
                    usage_data = chunk.get("usage", {})

        duration = time.monotonic() - start
        full_content = "".join(content_parts)

        # Qwen3 Think-Tags strippen (falls der Gateway sie nicht filtert)
        full_content = _strip_think_tags(full_content)

        # Logging mit Token-Counts für Budget-Tracking
        logger.info(
            "%s: model=%s, tokens=%s, duration=%.1fs, streaming=True",
            label,
            model,
            usage_data or "n/a",
            duration,
        )

        return full_content if full_content else None

    except httpx.ReadTimeout:
        # 120s kein Token → Ollama eingefroren
        logger.warning(
            "%s: Read-Timeout (120s kein Token) — Ollama vermutlich eingefroren. "
            "Fallback wird versucht.",
            label,
        )
        return None
    except httpx.ConnectTimeout:
        logger.warning("%s: Verbindungsaufbau fehlgeschlagen (30s)", label)
        return None
    except httpx.RemoteProtocolError as exc:
        # SSE-Verbindung abgebrochen (Gateway-Restart, Netzwerkfehler)
        logger.warning("%s: SSE-Verbindung abgebrochen: %s", label, exc)
        return None
    except (httpx.ConnectError, httpx.TimeoutException) as exc:
        logger.warning("%s nicht erreichbar: %s", label, exc)
        return None
    except Exception:
        logger.warning("%s Fehler", label, exc_info=True)
        return None
```

### 2. `_call_egpu_gateway()` anpassen

Diese Funktion ruft `_call_openai_compatible()` auf — da die Streaming-Logik dort implementiert ist, muss hier nur sichergestellt werden dass `max_tokens` immer gesetzt ist (der Gateway erzwingt inzwischen einen Default von 4096, aber defense-in-depth):

```python
def _call_egpu_gateway(...):
    resolved_model = model or settings.VP_AI_EGPU_MODEL
    if not max_tokens:
        max_tokens = 4000  # Pflicht für Qwen3 (Think-Overhead)

    # Primär: EVO X2 direkt
    evo_url = getattr(settings, "VP_AI_EVO_X2_URL", None)
    if evo_url:
        result = _call_openai_compatible(
            evo_url, resolved_model, system_prompt, user_prompt,
            temperature, max_tokens, "EVO X2",
        )
        if result is not None:
            return result

    # Fallback: eGPU Manager Gateway
    # ... (unverändert, nutzt ebenfalls _call_openai_compatible)
```

### 3. SSE-Format: Ollama vs. OpenAI

Der eGPU Manager Gateway normalisiert das Format bereits (raw byte passthrough seit dem letzten Update). Trotzdem gibt es Unterschiede die der Client kennen muss:

| Feld | OpenAI | Ollama (via Gateway) |
|------|--------|---------------------|
| `choices[0].delta.content` | Immer vorhanden | Vorhanden |
| `choices[0].delta.reasoning_content` | Nicht vorhanden | Qwen3: Think-Tokens |
| `choices[0].finish_reason` | `"stop"` | `"stop"` |
| `usage` im letzten Chunk | Ja (prompt_tokens, completion_tokens) | Teilweise (eval_count) |
| `data: [DONE]` | Immer am Ende | Immer am Ende |
| Leere Deltas (Heartbeats) | Selten | Häufig bei Qwen3 |

### 4. Qwen3 Think-Tags

Qwen3 sendet bei aktivem Think-Modus:
1. Zuerst `reasoning_content`-Chunks (der interne Reasoning-Prozess)
2. Dann `content`-Chunks (die eigentliche Antwort)

Der Client sammelt **nur `content`**, ignoriert `reasoning_content`. Falls der Gateway die Felder nicht sauber trennt und Think-Tags `<think>...</think>` im `content` landen, werden sie durch `_strip_think_tags()` auf dem Gesamttext entfernt.

Der `/no_think`-Suffix im Prompt deaktiviert den Think-Modus und spart ~50% Tokens. Für Batch-Jobs (Kompendium-Generierung) empfohlen.

### 5. Timeout-Strategie

Bei Streaming gibt es keinen klassischen Response-Timeout. Stattdessen:

| Timeout | Wert | Zweck |
|---------|------|-------|
| **Connect** | 30s | Verbindungsaufbau zum Gateway/Ollama |
| **Read** | 120s | Zwischen zwei SSE-Events. Wenn 120s kein Token kommt, ist Ollama eingefroren |
| **Gesamt** | Kein Limit | Streaming kann beliebig lang dauern (4000 Tokens bei 4.5 tok/s = 15 Min) |

Der Read-Timeout von 120s ist der zentrale Schutzmechanismus: Wenn Ollama einfriert (VRAM-Problem, Modell-Swap), kommen keine Tokens mehr. Nach 120s Stille wird die Verbindung abgebrochen und der Fallback-Provider versucht.

### 6. Memory bei Batch-Jobs

Bei Batch-Generierung (z.B. 100 Aufsätze à 2500 Tokens) sammelt der Client pro Request ~10-15 KB Text im RAM. Das ist unkritisch. Bei extremen Fällen (max_tokens=8000, 50 KB pro Antwort) wäre ein Streaming-to-File-Ansatz denkbar, ist aber für den aktuellen Use-Case nicht nötig.

### 7. Fallback-Verhalten bei Streaming-Fehler

Wenn die SSE-Verbindung abbricht (Netzwerkfehler, Gateway-Restart, Ollama-Freeze):

1. Bereits gesammelter Text wird **verworfen** (keine Teilantworten)
2. `_call_openai_compatible()` gibt `None` zurück
3. `call_llm()` wechselt zum nächsten Provider in der Fallback-Kette:
   - EVO X2 direkt → eGPU Manager Gateway → Ollama lokal → Anthropic → OpenAI
4. Nur bei Provider-Wechsel wird neu versucht, nicht beim selben Provider (verhindert Retry-Loops bei eingefrorenem Ollama)

## Dateien die geändert werden müssen

| Datei | Änderung |
|-------|----------|
| `backend/app/modules/vp_ai/services/llm_gateway_client.py` | `_call_openai_compatible()` auf `httpx.stream()` umstellen, SSE-Parsing, Read-Timeout 120s |
| Keine weiteren | `call_llm()` Signatur bleibt gleich, alle Consumer unverändert |

## Testplan

1. **Basis:** `call_llm("system", "Was ist EFRE? 2 Sätze.")` → Antwort in <30s statt Timeout
2. **Think-Tags:** Antwort enthält kein `<think>` — korrekt gestrippt
3. **Leere Deltas:** Keine leeren Strings in der Antwort
4. **KB Text Generator:** Aufsatz generieren via `/knowledge/articles/generate`
5. **RAG Ask:** `/vpai-notebook/rag/ask` mit Frage → Antwort mit Quellen
6. **Timeout:** Ollama stoppen → Read-Timeout nach 120s → Fallback auf Anthropic
7. **Token-Logging:** Log zeigt `tokens={prompt_tokens: X, completion_tokens: Y}`
8. **Smoke-Test:** `bash backend/tests/smoke_test_api.sh` → alle 49 Tests grün
9. **Batch:** `generate_kompendium.py --provider egpu_gateway --only zuwendung` → Aufsatz in <60s
