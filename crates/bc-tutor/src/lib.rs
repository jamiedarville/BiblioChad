//! The learning assistant.
//!
//! Given the passage on screen and what the reader has told us about
//! themselves, an LLM maps the passage onto Vygotsky's **Zone of Proximal
//! Development**:
//!
//! * **You've got this**: ideas the reader can already handle alone.
//! * **Your zone**: ideas just out of reach, each with graduated hints that
//!   go from a light nudge to a full explanation, plus a question to try.
//! * **Not yet**: ideas that need more groundwork first.
//!
//! The reader rates each map (too easy, just right, too hard) and the
//! `stretch` level moves so the next map stays in the zone.
//!
//! The user brings their own API key. Two providers are supported: the
//! Anthropic Messages API (default) and any OpenAI-compatible
//! `/chat/completions` endpoint (OpenAI, OpenRouter, Ollama, LM Studio, ...).

mod anthropic;
mod guide;
mod lab;
mod lab_prompt;
mod openai;
mod prompt;

use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use guide::{CheckQuestion, KnownIdea, LaterIdea, ZoneGuide, ZoneItem};
pub use lab::{sample_book_text, Lab, LabDiagram, LabGuide, LabStep};
pub use lab_prompt::{lab_schema, lab_system_prompt, lab_user_message, MAX_BOOK_CHARS};
pub use prompt::{system_prompt, user_message, zone_schema};

pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-opus-5-5";
pub const DEFAULT_ANTHROPIC_BASE: &str = "https://api.anthropic.com";
pub const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com/v1";

/// Longest passage sent to the provider, in characters.
pub const MAX_PASSAGE_CHARS: usize = 16_000;

pub const MIN_STRETCH: i64 = 1;
pub const MAX_STRETCH: i64 = 5;
pub const DEFAULT_STRETCH: i64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    Anthropic,
    /// Any server that speaks the OpenAI `/chat/completions` API.
    OpenaiCompatible,
}

/// Provider settings. The API key is stored separately, in the OS
/// credential store.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TutorConfig {
    #[serde(default)]
    pub provider: Provider,
    /// Empty means the provider default (Anthropic only).
    #[serde(default)]
    pub model: String,
    /// Empty means the provider's public endpoint.
    #[serde(default)]
    pub base_url: String,
}

impl TutorConfig {
    pub fn effective_model(&self) -> Option<String> {
        let m = self.model.trim();
        match (m.is_empty(), self.provider) {
            (false, _) => Some(m.to_string()),
            (true, Provider::Anthropic) => Some(DEFAULT_ANTHROPIC_MODEL.to_string()),
            (true, Provider::OpenaiCompatible) => None,
        }
    }

    pub fn effective_base_url(&self) -> String {
        let b = self.base_url.trim().trim_end_matches('/');
        if !b.is_empty() {
            return b.to_string();
        }
        match self.provider {
            Provider::Anthropic => DEFAULT_ANTHROPIC_BASE.into(),
            Provider::OpenaiCompatible => DEFAULT_OPENAI_BASE.into(),
        }
    }

    /// Check the settings before saving them.
    pub fn validate(&self) -> Result<()> {
        if self.effective_model().is_none() {
            return Err(TutorError::NotConfigured(
                "Enter the model name your provider uses.".into(),
            ));
        }
        let b = self.base_url.trim();
        if !b.is_empty() {
            let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"]
                .iter()
                .any(|p| b.starts_with(p));
            if !b.starts_with("https://") && !local {
                return Err(TutorError::NotConfigured(
                    "The endpoint must start with https:// (plain http is only allowed for localhost).".into(),
                ));
            }
        }
        Ok(())
    }
}

/// What the reader has told us about themselves for this book.
#[derive(Debug, Clone, Default)]
pub struct Learner {
    /// What they already know about the subject.
    pub background: String,
    /// Why they are reading this book.
    pub goal: String,
    /// 1 = lots of support .. 5 = maximum challenge.
    pub stretch: i64,
}

/// A map made earlier in the same book, so the next one builds on it.
#[derive(Debug, Clone)]
pub struct EarlierZone {
    pub label: String,
    pub concepts: Vec<String>,
    /// "too_easy" | "just_right" | "too_hard"
    pub feedback: Option<String>,
}

/// A highlight or note the reader made inside the passage.
#[derive(Debug, Clone)]
pub struct ReaderMark {
    pub text: String,
    pub note: Option<String>,
}

/// Everything the prompt is built from.
#[derive(Debug, Clone, Default)]
pub struct ZoneRequest {
    pub title: String,
    pub author: Option<String>,
    /// Where the reader is, e.g. "page 42" or "Chapter 3 · 41%".
    pub location: String,
    pub passage: String,
    pub learner: Learner,
    pub earlier: Vec<EarlierZone>,
    pub marks: Vec<ReaderMark>,
}

/// A finished map and the model that wrote it.
#[derive(Debug, Clone)]
pub struct Generated {
    pub guide: ZoneGuide,
    pub model: String,
}

/// Everything the "Build a lab" prompt is built from. `excerpt` is the
/// book text to analyse: the whole book when short, otherwise an even
/// sample produced by [`sample_book_text`].
#[derive(Debug, Clone, Default)]
pub struct LabRequest {
    pub title: String,
    pub author: Option<String>,
    /// "epub" / "pdf", for the model's awareness only.
    pub format: Option<String>,
    pub page_count: Option<u32>,
    /// Table of contents / outline, one entry per line.
    pub toc: String,
    pub excerpt: String,
    /// True when `excerpt` is a sample of a longer book.
    pub sampled: bool,
    pub learner: Learner,
    /// Ask for this many labs (1..=10); `None` lets the model choose 3–7.
    pub lab_count: Option<u32>,
    /// Optional extra steering from the reader.
    pub focus: String,
}

/// A finished lab guide and the model that wrote it.
#[derive(Debug, Clone)]
pub struct GeneratedLabs {
    pub guide: LabGuide,
    pub model: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TutorError {
    #[error("{0}")]
    NotConfigured(String),
    #[error(
        "The provider rejected the API key ({0}). Check the key in Settings → Learning assistant."
    )]
    Auth(String),
    #[error("The provider doesn't recognise that model or endpoint ({0}). Check the model name in Settings → Learning assistant.")]
    NotFound(String),
    #[error("The provider is busy or rate limited ({0}). Try again in a minute.")]
    Busy(String),
    #[error("The provider declined to map this passage{0}.")]
    Refused(String),
    #[error("The provider's answer was cut off before it finished. Try again, or choose a model with a larger output limit.")]
    Truncated,
    #[error("Provider error {status}: {message}")]
    Provider { status: u16, message: String },
    #[error("The provider's answer couldn't be read: {0}")]
    Invalid(String),
    #[error("Couldn't reach the provider: {0}")]
    Http(#[from] reqwest::Error),
}

impl TutorError {
    /// Short machine-readable kind for the UI.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotConfigured(_) => "not_configured",
            Self::Auth(_) => "tutor_auth",
            Self::NotFound(_) => "tutor_model",
            Self::Busy(_) => "tutor_busy",
            Self::Refused(_) => "tutor_refused",
            Self::Truncated => "tutor_truncated",
            Self::Provider { .. } | Self::Invalid(_) => "tutor",
            Self::Http(_) => "offline",
        }
    }
}

pub type Result<T> = std::result::Result<T, TutorError>;

/// A configured provider plus its key.
pub struct Tutor {
    http: reqwest::Client,
    config: TutorConfig,
    api_key: String,
    timeout: Duration,
    retry_base: Duration,
}

impl Tutor {
    pub fn new(http: reqwest::Client, config: TutorConfig, api_key: impl Into<String>) -> Self {
        Self {
            http,
            config,
            api_key: api_key.into(),
            timeout: Duration::from_secs(240),
            retry_base: Duration::from_millis(1500),
        }
    }

    /// Shorter waits between retries (tests).
    pub fn with_retry_base(mut self, base: Duration) -> Self {
        self.retry_base = base;
        self
    }

    fn model(&self) -> Result<String> {
        self.config
            .effective_model()
            .ok_or_else(|| TutorError::NotConfigured("No model is set.".into()))
    }

    /// Map the zone for one passage.
    pub async fn generate(&self, req: &ZoneRequest) -> Result<Generated> {
        if req.passage.trim().is_empty() {
            return Err(TutorError::NotConfigured(
                "There is no text on this page to work with.".into(),
            ));
        }
        let model = self.model()?;
        let system = system_prompt();
        let user = user_message(req);
        let schema = zone_schema();
        let (text, used_model) = self
            .with_retries(|| async {
                match self.config.provider {
                    Provider::Anthropic => {
                        anthropic::complete(self, &model, &system, &user, Some(&schema), 16_000)
                            .await
                    }
                    Provider::OpenaiCompatible => {
                        openai::complete(self, &model, &system, &user, Some(&schema), 16_000).await
                    }
                }
            })
            .await?;
        let guide = ZoneGuide::parse(&text)?;
        Ok(Generated {
            guide,
            model: used_model,
        })
    }

    /// Build a whole-book lab series ("Build a lab").
    pub async fn generate_labs(&self, req: &LabRequest) -> Result<GeneratedLabs> {
        if req.excerpt.trim().is_empty() {
            return Err(TutorError::NotConfigured(
                "There is no readable text in this book to build labs from.".into(),
            ));
        }
        let model = self.model()?;
        let system = lab_system_prompt();
        let user = lab_user_message(req);
        let schema = lab_schema();
        let (text, used_model) = self
            .with_retries(|| async {
                match self.config.provider {
                    Provider::Anthropic => {
                        anthropic::complete(self, &model, &system, &user, Some(&schema), 32_768)
                            .await
                    }
                    Provider::OpenaiCompatible => {
                        openai::complete(self, &model, &system, &user, Some(&schema), 32_768).await
                    }
                }
            })
            .await?;
        let guide = LabGuide::parse(&text)?;
        Ok(GeneratedLabs {
            guide,
            model: used_model,
        })
    }

    /// Cheapest request that proves the key, model and endpoint work.
    pub async fn ping(&self) -> Result<String> {
        let model = self.model()?;
        match self.config.provider {
            Provider::Anthropic => anthropic::ping(self, &model).await?,
            Provider::OpenaiCompatible => openai::ping(self, &model).await?,
        }
        Ok(model)
    }

    async fn with_retries<T, F, Fut>(&self, mut f: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        const ATTEMPTS: u32 = 3;
        let mut attempt = 1;
        loop {
            match f().await {
                Err(e) if attempt < ATTEMPTS && retryable(&e) => {
                    let wait = self.retry_base * 2u32.pow(attempt - 1);
                    tracing::info!(attempt, "tutor request failed, retrying: {e}");
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                other => return other,
            }
        }
    }
}

fn retryable(e: &TutorError) -> bool {
    match e {
        TutorError::Busy(_) => true,
        TutorError::Provider { status, .. } => *status >= 500,
        TutorError::Http(e) => e.is_timeout() || e.is_connect(),
        _ => false,
    }
}

/// Map an HTTP error status and body to an error the reader can act on.
pub(crate) fn status_error(status: u16, body: &str) -> TutorError {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("message"))
                .or_else(|| v.get("error"))
                .and_then(|m| m.as_str().map(str::to_string))
        })
        .unwrap_or_else(|| {
            let t = body.trim();
            if t.is_empty() {
                format!("HTTP {status}")
            } else {
                t.chars().take(300).collect()
            }
        });
    match status {
        401 | 403 => TutorError::Auth(message),
        404 => TutorError::NotFound(message),
        408 | 409 | 429 | 529 => TutorError::Busy(message),
        _ => TutorError::Provider { status, message },
    }
}

/// Cut `s` to at most `max` characters on a char boundary.
pub fn clip(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_and_validation() {
        let c = TutorConfig::default();
        assert_eq!(
            c.effective_model().as_deref(),
            Some(DEFAULT_ANTHROPIC_MODEL)
        );
        assert_eq!(c.effective_base_url(), DEFAULT_ANTHROPIC_BASE);
        assert!(c.validate().is_ok());

        let o = TutorConfig {
            provider: Provider::OpenaiCompatible,
            ..Default::default()
        };
        assert!(o.validate().is_err(), "compatible endpoints need a model");
        let o = TutorConfig {
            model: "llama3".into(),
            base_url: "http://localhost:11434/v1/".into(),
            ..o
        };
        assert!(o.validate().is_ok());
        assert_eq!(o.effective_base_url(), "http://localhost:11434/v1");

        let bad = TutorConfig {
            base_url: "http://example.com".into(),
            ..Default::default()
        };
        assert!(bad.validate().is_err(), "remote plain http is refused");
    }

    #[test]
    fn status_errors_are_actionable() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        assert!(matches!(status_error(401, body), TutorError::Auth(m) if m == "invalid x-api-key"));
        assert!(matches!(status_error(529, ""), TutorError::Busy(_)));
        assert!(matches!(status_error(404, "nope"), TutorError::NotFound(m) if m == "nope"));
        assert!(matches!(
            status_error(500, "{}"),
            TutorError::Provider { status: 500, .. }
        ));
    }

    #[test]
    fn clip_respects_char_boundaries() {
        assert_eq!(clip("héllo", 2), "hé");
        assert_eq!(clip("hi", 10), "hi");
    }
}
