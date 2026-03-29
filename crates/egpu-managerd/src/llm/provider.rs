use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use super::types::{ChatCompletionRequest, ChatCompletionResponse};

/// Fehlertyp fuer Provider-Operationen
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error: {status} {message}")]
    Api { status: u16, message: String },
}

/// Raw SSE byte stream — upstream-Bytes werden 1:1 durchgereicht.
/// Bewahrt alle Felder (reasoning_content, tool_calls etc.) ohne Parse/Re-serialize.
pub type SseByteStream =
    Pin<Box<dyn Stream<Item = Result<bytes::Bytes, ProviderError>> + Send>>;

/// Trait fuer LLM-Provider
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Provider-Name
    fn name(&self) -> &str;

    /// Prueft ob dieser Provider das angegebene Modell unterstuetzt
    fn supports_model(&self, model: &str) -> bool;

    /// Chat-Completion-Request senden (nicht-streamend)
    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError>;

    /// Chat-Completion-Request als SSE-Stream (raw byte passthrough).
    /// Default: nicht unterstuetzt — Provider muss dies explizit implementieren.
    async fn chat_completion_stream(
        &self,
        _request: &ChatCompletionRequest,
    ) -> Result<SseByteStream, ProviderError> {
        Err(ProviderError::Api {
            status: 501,
            message: format!("Provider '{}' unterstuetzt kein Streaming", self.name()),
        })
    }

    /// Health-Check
    async fn health_check(&self) -> bool;
}
