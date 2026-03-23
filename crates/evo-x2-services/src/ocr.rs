use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use tracing::{info, warn};

const MAX_UPLOAD: usize = 100 * 1024 * 1024; // 100 MB

#[derive(Serialize)]
struct OcrResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pages: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

pub async fn serve(host: &str, port: u16) -> anyhow::Result<()> {
    // Pruefe ob docling verfuegbar ist
    check_docling();

    let app = Router::new()
        .route("/health", get(handle_health))
        .route("/ocr", post(handle_ocr));

    let addr: std::net::SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle_health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn handle_ocr(
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<OcrResponse>) {
    if body.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(OcrResponse {
            text: None, pages: None,
            error: Some("empty body".into()),
        }));
    }

    if body.len() > MAX_UPLOAD {
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(OcrResponse {
            text: None, pages: None,
            error: Some("upload too large".into()),
        }));
    }

    // Dateiendung aus Content-Type bestimmen
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/pdf");
    let ext = if ct.contains("pdf") { ".pdf" } else { ".png" };

    // Temp-Datei schreiben
    let tmp_path = format!("/tmp/evo-ocr-{}{ext}", std::process::id());
    if let Err(e) = std::fs::write(&tmp_path, &body) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(OcrResponse {
            text: None, pages: None,
            error: Some(format!("temp file: {e}")),
        }));
    }

    // Docling via Python aufrufen (da Docling eine Python-Library ist)
    let result = run_docling(&tmp_path).await;

    // Cleanup
    let _ = std::fs::remove_file(&tmp_path);

    match result {
        Ok((text, pages)) => {
            info!("OCR erfolgreich: {} Zeichen, {} Seiten", text.len(), pages);
            (StatusCode::OK, Json(OcrResponse {
                text: Some(text),
                pages: Some(pages),
                error: None,
            }))
        }
        Err(e) => {
            warn!("OCR fehlgeschlagen: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(OcrResponse {
                text: None, pages: None,
                error: Some(e.to_string()),
            }))
        }
    }
}

async fn run_docling(file_path: &str) -> anyhow::Result<(String, i32)> {
    // Docling ist eine Python-Library — wir rufen sie als Subprocess auf.
    // Das ist der einzig sinnvolle Weg, da Docling kein Rust-Binding hat.
    let python_script = format!(
        r#"
import json, sys
try:
    from docling.document_converter import DocumentConverter
    converter = DocumentConverter()
    result = converter.convert("{path}")
    text = result.document.export_to_markdown()
    try:
        pages = len(result.document.pages)
    except (AttributeError, TypeError):
        pages = -1
    print(json.dumps({{"text": text, "pages": pages}}))
except Exception as e:
    print(json.dumps({{"error": str(e)}}), file=sys.stderr)
    sys.exit(1)
"#,
        path = file_path.replace('"', r#"\""#)
    );

    let output = tokio::process::Command::new("python3")
        .args(["-c", &python_script])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("python3 nicht verfuegbar: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Docling fehlgeschlagen: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("Docling-Output ungueltig: {e}"))?;

    let text = parsed["text"].as_str().unwrap_or("").to_string();
    let pages = parsed["pages"].as_i64().unwrap_or(-1) as i32;

    Ok((text, pages))
}

fn check_docling() {
    let result = std::process::Command::new("python3")
        .args(["-c", "import docling; print(docling.__version__)"])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            let ver = String::from_utf8_lossy(&output.stdout);
            info!("Docling v{} verfuegbar", ver.trim());
        }
        _ => {
            warn!("Docling nicht installiert! /ocr wird fehlschlagen.");
            warn!("  pip install 'docling>=2.0,<3.0'");
        }
    }
}
