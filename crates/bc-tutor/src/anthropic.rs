//! Anthropic Messages API (raw HTTP; there is no official Rust SDK).

use serde_json::{json, Value};

use crate::{status_error, Result, Tutor, TutorError, DEFAULT_ANTHROPIC_BASE};

const API_VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Models that accept `fallbacks: "default"`: if a safety classifier
/// declines, the API re-runs the request on a suitable model instead of
/// returning a refusal.
fn supports_default_fallback(model: &str) -> bool {
    matches!(
        model,
        "claude-opus-5-5" | "claude-opus-5" | "claude-fable-5-1" | "claude-sonnet-5-5"
    )
}

fn request(t: &Tutor, body: &Value, fallback: bool) -> reqwest::RequestBuilder {
    let url = format!("{}/v1/messages", t.config.effective_base_url());
    let mut rb = t
        .http
        .post(url)
        .timeout(t.timeout)
        .header("x-api-key", &t.api_key)
        .header("anthropic-version", API_VERSION)
        .json(body);
    if fallback {
        rb = rb.header("anthropic-beta", FALLBACK_BETA);
    }
    rb
}

/// Returns the text of the answer and the model that produced it.
pub(crate) async fn complete(
    t: &Tutor,
    model: &str,
    system: &str,
    user: &str,
    schema: Option<&Value>,
    max_tokens: u32,
) -> Result<(String, String)> {
    // Fallbacks are a Claude API feature; skip them behind custom proxies.
    let fallback =
        supports_default_fallback(model) && t.config.effective_base_url() == DEFAULT_ANTHROPIC_BASE;
    let mut body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": [{ "type": "text", "text": system, "cache_control": { "type": "ephemeral" } }],
        "messages": [{ "role": "user", "content": user }],
    });
    if let Some(schema) = schema {
        body["output_config"] = json!({ "format": { "type": "json_schema", "schema": schema } });
    }
    if fallback {
        body["fallbacks"] = json!("default");
    }
    let res = request(t, &body, fallback).send().await?;
    let status = res.status().as_u16();
    let text = res.text().await?;
    if !(200..300).contains(&status) {
        return Err(status_error(status, &text));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| TutorError::Invalid(e.to_string()))?;
    match v["stop_reason"].as_str() {
        Some("refusal") => {
            let why = v
                .pointer("/stop_details/explanation")
                .and_then(Value::as_str)
                .map(|e| format!(": {e}"))
                .unwrap_or_default();
            return Err(TutorError::Refused(why));
        }
        Some("max_tokens") => return Err(TutorError::Truncated),
        _ => {}
    }
    let answer: String = v["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    if answer.trim().is_empty() {
        return Err(TutorError::Invalid("the answer was empty".into()));
    }
    let used = v["model"].as_str().unwrap_or(model).to_string();
    Ok((answer, used))
}

pub(crate) async fn ping(t: &Tutor, model: &str) -> Result<()> {
    let body = json!({
        "model": model,
        "max_tokens": 32,
        "messages": [{ "role": "user", "content": "Reply with OK." }],
    });
    let res = request(t, &body, false).send().await?;
    let status = res.status().as_u16();
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(status_error(status, &res.text().await.unwrap_or_default()))
    }
}
