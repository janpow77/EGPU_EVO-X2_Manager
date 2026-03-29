use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use tracing::{debug, warn};

use crate::llm::provider::{LlmProvider, ProviderError, SseByteStream};
use crate::llm::types::*;

/// OpenAI-kompatibler Provider (funktioniert mit Ollama, xAI/Grok, DeepSeek, Zhipu
/// und jeder anderen OpenAI-kompatiblen API)
pub struct OpenAiCompatProvider {
    name: String,
    base_url: String,
    api_key: Option<String>,
    models: Vec<String>,
    client: Client,
}

impl OpenAiCompatProvider {
    pub fn new(
        name: String,
        base_url: String,
        api_key: Option<String>,
        models: Vec<String>,
        timeout_seconds: u64,
    ) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_seconds))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            name,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            models,
            client,
        }
    }

    fn build_request(&self, request: &ChatCompletionRequest) -> reqwest::RequestBuilder {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let mut req = self.client.post(&url).json(request);
        if let Some(ref key) = self.api_key {
            req = req.bearer_auth(key);
        }
        req
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn supports_model(&self, model: &str) -> bool {
        // Leere Modell-Liste bedeutet: alle Modelle werden akzeptiert
        self.models.is_empty() || self.models.iter().any(|m| m == model)
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError> {
        debug!("OpenAI-compat request to {} model={}", self.name, request.model);

        let resp = self.build_request(request).send().await?;
        let status = resp.status().as_u16();

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                status,
                message: body,
            });
        }

        let mut response: ChatCompletionResponse = resp.json().await?;
        response.provider = self.name.clone();
        Ok(response)
    }

    async fn chat_completion_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<SseByteStream, ProviderError> {
        debug!(
            "OpenAI-compat STREAM request to {} model={}",
            self.name, request.model
        );

        // Streaming braucht laengeres Timeout (LLM-Generierung kann Minuten dauern)
        let resp = self
            .build_request(request)
            .timeout(std::time::Duration::from_secs(1800))
            .send()
            .await?;
        let status = resp.status().as_u16();

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                status,
                message: body,
            });
        }

        // Content-Type pruefen — Upstream muss SSE liefern
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !content_type.contains("text/event-stream")
            && !content_type.contains("text/plain")
            && !content_type.contains("application/x-ndjson")
        {
            return Err(ProviderError::Api {
                status: 502,
                message: format!(
                    "Upstream liefert kein SSE (Content-Type: '{content_type}')"
                ),
            });
        }

        debug!(
            "Streaming-Verbindung zu {} hergestellt (Content-Type: {})",
            self.name, content_type
        );

        // Raw byte passthrough — keine JSON-Deserialisierung.
        // Bewahrt alle Felder 1:1 (reasoning_content, tool_calls, etc.)
        Ok(Box::pin(
            resp.bytes_stream()
                .map(|r| r.map_err(ProviderError::Http)),
        ))
    }

    async fn health_check(&self) -> bool {
        let url = format!("{}/v1/models", self.base_url);
        let mut req = self.client.get(&url);
        if let Some(ref key) = self.api_key {
            req = req.bearer_auth(key);
        }
        match req.timeout(std::time::Duration::from_secs(5)).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(e) => {
                warn!("Health check failed for {}: {}", self.name, e);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use std::pin::Pin;

    #[test]
    fn test_supports_model_empty_list() {
        let provider = OpenAiCompatProvider::new(
            "test".into(),
            "http://localhost:11434".into(),
            None,
            vec![],
            120,
        );
        assert!(provider.supports_model("any-model"));
    }

    #[test]
    fn test_supports_model_specific_list() {
        let provider = OpenAiCompatProvider::new(
            "test".into(),
            "http://localhost:11434".into(),
            None,
            vec!["qwen3:14b".into(), "llama3:8b".into()],
            120,
        );
        assert!(provider.supports_model("qwen3:14b"));
        assert!(!provider.supports_model("gpt-4"));
    }

    #[test]
    fn test_base_url_trailing_slash_stripped() {
        let provider = OpenAiCompatProvider::new(
            "test".into(),
            "http://localhost:11434/".into(),
            None,
            vec![],
            120,
        );
        assert_eq!(provider.base_url, "http://localhost:11434");
    }

    // --- SSE-Format-Validierungstests ---
    // Diese testen unser Verstaendnis des SSE-Formats, nicht den Produktionspfad
    // (der macht raw byte passthrough ohne Parsing).

    fn mock_byte_stream(
        chunks: Vec<&str>,
    ) -> Pin<Box<impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>>>> {
        Box::pin(stream::iter(
            chunks
                .into_iter()
                .map(|s| Ok(bytes::Bytes::from(s.to_string())))
                .collect::<Vec<_>>(),
        ))
    }

    /// Minimaler SSE-Parser fuer Tests — parsed data:-Zeilen zu JSON-Values.
    /// Stoppt bei [DONE] (wie ein korrekter OpenAI-Client).
    async fn collect_sse_events(chunks: Vec<&str>) -> Vec<serde_json::Value> {
        let byte_stream = mock_byte_stream(chunks);
        let all_bytes: Vec<bytes::Bytes> = byte_stream
            .filter_map(|r| async { r.ok() })
            .collect()
            .await;

        let full_text: String = all_bytes
            .iter()
            .map(|b| String::from_utf8_lossy(b).to_string())
            .collect();

        full_text
            .replace("\r\n", "\n")
            .split("\n\n")
            .map(|event| {
                event
                    .lines()
                    .filter_map(|l| {
                        let l = l.trim_start();
                        if l.starts_with(':') {
                            return None;
                        }
                        l.strip_prefix("data: ").or_else(|| l.strip_prefix("data:"))
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
            .take_while(|data| data.trim() != "[DONE]")
            .filter(|data| !data.is_empty())
            .filter_map(|data| serde_json::from_str(&data).ok())
            .collect()
    }

    #[tokio::test]
    async fn test_sse_format_valid_chunks() {
        let sse_data = vec![
            "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n",
            "data: [DONE]\n\n",
        ];

        let events = collect_sse_events(sse_data).await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["choices"][0]["delta"]["role"], "assistant");
        assert_eq!(events[1]["choices"][0]["delta"]["content"], "Hi");
    }

    #[tokio::test]
    async fn test_sse_format_reasoning_content_preserved() {
        // qwen3/qwq sendet reasoning_content ohne content — raw passthrough bewahrt das
        let sse_data = vec![
            "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"qwen3:14b\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"Lass mich nachdenken...\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"qwen3:14b\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"},\"finish_reason\":null}]}\n\n",
            "data: [DONE]\n\n",
        ];

        let events = collect_sse_events(sse_data).await;
        assert_eq!(events.len(), 2);
        // reasoning_content ist vorhanden (wuerde bei typed Parse verloren gehen)
        assert_eq!(
            events[0]["choices"][0]["delta"]["reasoning_content"],
            "Lass mich nachdenken..."
        );
        assert_eq!(events[1]["choices"][0]["delta"]["content"], "OK");
    }

    #[tokio::test]
    async fn test_sse_format_done_terminates() {
        let sse_data = vec![
            "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"X\"},\"finish_reason\":null}]}\n\n",
            "data: [DONE]\n\n",
            "data: {\"id\":\"c2\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Y\"},\"finish_reason\":null}]}\n\n",
        ];

        let events = collect_sse_events(sse_data).await;
        // [DONE] beendet die Auswertung, danach folgende Events werden ignoriert
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["choices"][0]["delta"]["content"], "X");
    }

    #[tokio::test]
    async fn test_sse_format_keepalive_ignored() {
        let sse_data = vec![
            ": keep-alive\n\n",
            "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n",
            "data: [DONE]\n\n",
        ];

        let events = collect_sse_events(sse_data).await;
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn test_sse_format_split_across_byte_chunks() {
        let sse_data = vec![
            "data: {\"id\":\"c1\",\"object\":",
            "\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Split\"},\"finish_reason\":null}]}\n\n",
            "data: [DONE]\n\n",
        ];

        let events = collect_sse_events(sse_data).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["choices"][0]["delta"]["content"], "Split");
    }

    #[tokio::test]
    async fn test_sse_format_finish_reason() {
        let sse_data = vec![
            "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        ];

        let events = collect_sse_events(sse_data).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["choices"][0]["finish_reason"], "stop");
    }

    #[tokio::test]
    async fn test_sse_format_crlf_normalization() {
        let sse_data = vec![
            "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"},\"finish_reason\":null}]}\r\n\r\n",
            "data: [DONE]\r\n\r\n",
        ];

        let events = collect_sse_events(sse_data).await;
        assert_eq!(events.len(), 1);
    }
}
