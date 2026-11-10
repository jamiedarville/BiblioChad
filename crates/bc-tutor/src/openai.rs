//! OpenAI-compatible `/chat/completions` (OpenAI, OpenRouter, Ollama,
//! LM Studio, ...).

use serde_json::{json, Value};

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

#[allow(clippy::too_many_arguments)]
pub(crate) async fn complete(
    t: &Tutor,
    model: &str,
    system: &str,
    user: &str,
    schema: Option<&Value>,
    _max_tokens: u32,
) -> Result<(String, String)> {
    let mut body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
    });
    if let Some(schema) = schema {
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": { "name": "zone_map", "strict": true, "schema": schema }
        });
    }
    let res = request(t, &body).send().await?;
    let status = res.status().as_u16();
    let text = res.text().await?;
    if !(200..300).contains(&status) {
        return Err(status_error(status, &text));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| TutorError::Invalid(e.to_string()))?;
    let choice = &v["choices"][0];
    if let Some(r) = choice.pointer("/message/refusal").and_then(Value::as_str) {
        return Err(TutorError::Refused(format!(": {r}")));
    }
    if choice["finish_reason"] == "length" {
        return Err(TutorError::Truncated);
    }
    let answer = choice
        .pointer("/message/content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if answer.trim().is_empty() {
        return Err(TutorError::Invalid("the answer was empty".into()));
    }
    let used = v["model"].as_str().unwrap_or(model).to_string();
    Ok((answer, used))
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
