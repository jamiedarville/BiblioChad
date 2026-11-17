//! OpenAI-compatible `/chat/completions` (OpenAI, OpenRouter, Ollama,
//! LM Studio, ...).

use serde_json::{json, Value};

use crate::chat::{ChatMessage, SseBuffer};
use crate::{status_error, Result, Tutor, TutorError};

fn request(t: &Tutor, body: &Value) -> reqwest::RequestBuilder {
    let url = format!("{}/chat/completions", t.config.effective_base_url());
    let mut rb = t.http.post(url).timeout(t.timeout).json(body);
    // Local servers often need no key.
    if !t.api_key.trim().is_empty() {
        rb = rb.bearer_auth(&t.api_key);
    }
    rb
}

fn body_for(
    model: &str,
    system: &str,
    messages: &[ChatMessage],
    schema: Option<&Value>,
    stream: bool,
) -> Value {
    let mut all = vec![json!({ "role": "system", "content": system })];
    all.extend(
        messages
            .iter()
            .map(|m| json!({ "role": m.role.as_str(), "content": m.content })),
    );
    let mut body = json!({ "model": model, "messages": all });
    if let Some(schema) = schema {
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": { "name": "zone_map", "strict": true, "schema": schema }
        });
    }
    if stream {
        body["stream"] = json!(true);
    }
    body
}

/// Parse a non-streaming response: (text, model, truncated).
fn parse_response(text: &str, model: &str) -> Result<(String, String, bool)> {
    let v: Value = serde_json::from_str(text).map_err(|e| TutorError::Invalid(e.to_string()))?;
    let choice = &v["choices"][0];
    if let Some(r) = choice.pointer("/message/refusal").and_then(Value::as_str) {
        return Err(TutorError::Refused(format!(": {r}")));
    }
    let truncated = choice["finish_reason"] == "length";
    let answer = choice
        .pointer("/message/content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if answer.trim().is_empty() {
        return Err(TutorError::Invalid("the answer was empty".into()));
    }
    let used = v["model"].as_str().unwrap_or(model).to_string();
    Ok((answer, used, truncated))
}

pub(crate) async fn complete_messages(
    t: &Tutor,
    model: &str,
    system: &str,
    messages: &[ChatMessage],
    schema: Option<&Value>,
    _max_tokens: u32,
) -> Result<(String, String, bool)> {
    // No length cap: newer OpenAI models reject `max_tokens`, and some
    // compatible servers don't know `max_completion_tokens` (see ping).
    let body = body_for(model, system, messages, schema, false);
    let res = request(t, &body).send().await?;
    let status = res.status().as_u16();
    let text = res.text().await?;
    if !(200..300).contains(&status) {
        return Err(status_error(status, &text));
    }
    parse_response(&text, model)
}

/// Streaming variant. A 2xx reply that is not an event stream (a server
/// ignoring `stream: true`) is parsed as a normal response and delivered
/// as one delta.
pub(crate) async fn complete_messages_stream(
    t: &Tutor,
    model: &str,
    system: &str,
    messages: &[ChatMessage],
    on_delta: &mut (dyn FnMut(&str) -> bool + Send),
) -> Result<(String, String, bool)> {
    let body = body_for(model, system, messages, None, true);
    let mut res = request(t, &body).send().await?;
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
    let mut done = false;
    let handle = |payload: &str,
                  out: &mut String,
                  used: &mut String,
                  truncated: &mut bool,
                  done: &mut bool,
                  on_delta: &mut (dyn FnMut(&str) -> bool + Send)|
     -> Result<()> {
        if payload.trim() == "[DONE]" {
            *done = true;
            return Ok(());
        }
        let v: Value =
            serde_json::from_str(payload).map_err(|e| TutorError::Invalid(e.to_string()))?;
        if let Some(m) = v["model"].as_str() {
            *used = m.to_string();
        }
        if v.get("error").is_some_and(|e| !e.is_null()) {
            let msg = v
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("stream error")
                .to_string();
            return Err(TutorError::Invalid(msg));
        }
        let choice = &v["choices"][0];
        if let Some(r) = choice.pointer("/delta/refusal").and_then(Value::as_str) {
            if !r.is_empty() {
                return Err(TutorError::Refused(format!(": {r}")));
            }
        }
        // Only `content` is shown; role-only and reasoning_content deltas
        // from local servers are intentionally ignored.
        if let Some(text) = choice.pointer("/delta/content").and_then(Value::as_str) {
            if !text.is_empty() {
                out.push_str(text);
                if !on_delta(text) {
                    return Err(TutorError::Cancelled);
                }
            }
        }
        match choice["finish_reason"].as_str() {
            Some("length") => {
                *truncated = true;
                *done = true;
            }
            Some("stop") | Some("tool_calls") | Some("content_filter") => *done = true,
            _ => {}
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
                &mut done,
                on_delta,
            )?;
        }
        if done {
            break;
        }
    }
    if !done {
        for payload in sse.finish() {
            handle(
                &payload,
                &mut out,
                &mut used,
                &mut truncated,
                &mut done,
                on_delta,
            )?;
        }
    }
    if !done && out.is_empty() {
        return Err(TutorError::Invalid("the stream ended early".into()));
    }
    if out.trim().is_empty() {
        return Err(TutorError::Invalid("the answer was empty".into()));
    }
    Ok((out, used, truncated))
}

#[allow(clippy::too_many_arguments)]
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
        "messages": [{ "role": "user", "content": "Reply with OK." }],
        // No length cap: newer OpenAI models reject `max_tokens`, and some
        // compatible servers don't know `max_completion_tokens`.
    });
    let res = request(t, &body).send().await?;
    let status = res.status().as_u16();
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(status_error(status, &res.text().await.unwrap_or_default()))
    }
}
