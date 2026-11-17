//! Anthropic Messages API (raw HTTP; there is no official Rust SDK).

use serde_json::{json, Value};

use crate::chat::{ChatMessage, SseBuffer};
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

fn use_fallback(t: &Tutor, model: &str) -> bool {
    // Fallbacks are a Claude API feature; skip them behind custom proxies.
    supports_default_fallback(model) && t.config.effective_base_url() == DEFAULT_ANTHROPIC_BASE
}

fn body_for(
    model: &str,
    system: &str,
    messages: &[ChatMessage],
    schema: Option<&Value>,
    max_tokens: u32,
    fallback: bool,
    stream: bool,
) -> Value {
    let mut body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": [{ "type": "text", "text": system, "cache_control": { "type": "ephemeral" } }],
        "messages": messages.iter()
            .map(|m| json!({ "role": m.role.as_str(), "content": m.content }))
            .collect::<Vec<_>>(),
    });
    if let Some(schema) = schema {
        body["output_config"] = json!({ "format": { "type": "json_schema", "schema": schema } });
    }
    if fallback {
        body["fallbacks"] = json!("default");
    }
    if stream {
        body["stream"] = json!(true);
    }
    body
}

/// Parse a non-streaming response: (text, model, truncated).
fn parse_response(text: &str, model: &str) -> Result<(String, String, bool)> {
    let v: Value = serde_json::from_str(text).map_err(|e| TutorError::Invalid(e.to_string()))?;
    if v["stop_reason"].as_str() == Some("refusal") {
        let why = v
            .pointer("/stop_details/explanation")
            .and_then(Value::as_str)
            .map(|e| format!(": {e}"))
            .unwrap_or_default();
        return Err(TutorError::Refused(why));
    }
    let truncated = v["stop_reason"].as_str() == Some("max_tokens");
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
    Ok((answer, used, truncated))
}

/// Returns the text, the model, and whether the answer was cut off.
pub(crate) async fn complete_messages(
    t: &Tutor,
    model: &str,
    system: &str,
    messages: &[ChatMessage],
    schema: Option<&Value>,
    max_tokens: u32,
) -> Result<(String, String, bool)> {
    let fallback = use_fallback(t, model);
    let body = body_for(model, system, messages, schema, max_tokens, fallback, false);
    let res = request(t, &body, fallback).send().await?;
    let status = res.status().as_u16();
    let text = res.text().await?;
    if !(200..300).contains(&status) {
        return Err(status_error(status, &text));
    }
    parse_response(&text, model)
}

/// Streaming variant. A 2xx reply that is not an event stream (a proxy or
/// compatible server ignoring `stream: true`) is parsed as a normal
/// response and delivered as one delta.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn complete_messages_stream(
    t: &Tutor,
    model: &str,
    system: &str,
    messages: &[ChatMessage],
    max_tokens: u32,
    on_delta: &mut (dyn FnMut(&str) -> bool + Send),
) -> Result<(String, String, bool)> {
    let fallback = use_fallback(t, model);
    let body = body_for(model, system, messages, None, max_tokens, fallback, true);
    let mut res = request(t, &body, fallback).send().await?;
    let status = res.status().as_u16();
    if !(200..300).contains(&status) {
        let text = res.text().await.unwrap_or_default();
        return Err(status_error(status, &text));
    }
    let is_sse = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|c| c.contains("text/event-stream"))
        .unwrap_or(false);
    if !is_sse {
        let text = res.text().await?;
        let parsed = parse_response(&text, model)?;
        if !on_delta(&parsed.0) {
            return Err(TutorError::Cancelled);
        }
        return Ok(parsed);
    }

    let mut sse = SseBuffer::default();
    let mut out = String::new();
    let mut used = model.to_string();
    let mut truncated = false;
    let mut stopped = false;
    let handle = |payload: &str,
                  out: &mut String,
                  used: &mut String,
                  truncated: &mut bool,
                  stopped: &mut bool,
                  on_delta: &mut (dyn FnMut(&str) -> bool + Send)|
     -> Result<()> {
        let v: Value =
            serde_json::from_str(payload).map_err(|e| TutorError::Invalid(e.to_string()))?;
        match v["type"].as_str() {
            Some("message_start") => {
                if let Some(m) = v.pointer("/message/model").and_then(Value::as_str) {
                    *used = m.to_string();
                }
            }
            Some("content_block_delta")
                if v.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") =>
            {
                if let Some(text) = v.pointer("/delta/text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        out.push_str(text);
                        if !on_delta(text) {
                            return Err(TutorError::Cancelled);
                        }
                    }
                }
            }
            Some("message_delta") => {
                match v.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    Some("refusal") => return Err(TutorError::Refused(String::new())),
                    Some("max_tokens") => *truncated = true,
                    _ => {}
                }
            }
            Some("message_stop") => *stopped = true,
            Some("error") => {
                let kind = v
                    .pointer("/error/type")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let msg = v
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("stream error")
                    .to_string();
                return Err(match kind {
                    "overloaded_error" | "rate_limit_error" => TutorError::Busy(msg),
                    "authentication_error" => TutorError::Auth(msg),
                    "not_found_error" => TutorError::NotFound(msg),
                    _ => TutorError::Invalid(msg),
                });
            }
            _ => {} // ping, content_block_start/stop, thinking deltas
        }
        Ok(())
    };
    while let Some(chunk) = res.chunk().await? {
        for payload in sse.push(&chunk) {
            handle(
                &payload,
                &mut out,
                &mut used,
                &mut truncated,
                &mut stopped,
                on_delta,
            )?;
        }
        if stopped {
            break;
        }
    }
    if !stopped {
        for payload in sse.finish() {
            handle(
                &payload,
                &mut out,
                &mut used,
                &mut truncated,
                &mut stopped,
                on_delta,
            )?;
        }
    }
    if !stopped && out.is_empty() {
        return Err(TutorError::Invalid("the stream ended early".into()));
    }
    if out.trim().is_empty() {
        return Err(TutorError::Invalid("the answer was empty".into()));
    }
    Ok((out, used, truncated))
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
    let messages = [ChatMessage::user(user)];
    let (text, used, truncated) =
        complete_messages(t, model, system, &messages, schema, max_tokens).await?;
    if truncated {
        return Err(TutorError::Truncated);
    }
    Ok((text, used))
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
