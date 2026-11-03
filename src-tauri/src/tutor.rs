//! Learning assistant commands: provider settings, per-book learner
//! profiles, and Zone of Proximal Development maps.

use std::sync::Arc;

use bc_core::{BookFormat, Locator};
use bc_drive::TokenStore;
use bc_library::{Library, ZoneFeedback, ZoneMap, ZoneProfile};
use bc_tutor::{EarlierZone, Learner, Provider, ReaderMark, Tutor, TutorConfig, ZoneRequest};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::error::{CmdError, CmdResult};
use crate::state::AppState;

const CONFIG_KEY: &str = "tutor_config";
const KEY_HINT_KEY: &str = "tutor_key_hint";
/// Android has no desktop keyring; the key lives in the app's private
/// database instead, which other apps can't read.
#[cfg(target_os = "android")]
const ANDROID_KEY_SETTING: &str = "tutor_api_key";

/// Where the API key is kept.
pub fn key_store(_lib: &Arc<Library>) -> Arc<dyn TokenStore> {
    #[cfg(not(target_os = "android"))]
    return Arc::new(bc_drive::KeyringStore::new("BiblioChad", "llm-api-key"));
    #[cfg(target_os = "android")]
    return Arc::new(SettingsStore(_lib.clone()));
}

#[cfg(target_os = "android")]
struct SettingsStore(Arc<Library>);

#[cfg(target_os = "android")]
impl TokenStore for SettingsStore {
    fn load(&self) -> bc_drive::Result<Option<String>> {
        self.0
            .setting(ANDROID_KEY_SETTING)
            .map_err(|e| bc_drive::DriveError::Keyring(e.to_string()))
    }
    fn save(&self, key: &str) -> bc_drive::Result<()> {
        self.0
            .set_setting(ANDROID_KEY_SETTING, key)
            .map_err(|e| bc_drive::DriveError::Keyring(e.to_string()))
    }
    fn clear(&self) -> bc_drive::Result<()> {
        self.0
            .delete_setting(ANDROID_KEY_SETTING)
            .map_err(|e| bc_drive::DriveError::Keyring(e.to_string()))
    }
}

impl From<bc_tutor::TutorError> for CmdError {
    fn from(e: bc_tutor::TutorError) -> Self {
        Self::new(e.kind(), e.to_string())
    }
}

fn config(state: &AppState) -> CmdResult<TutorConfig> {
    Ok(state
        .lib
        .setting_json::<TutorConfig>(CONFIG_KEY)?
        .unwrap_or_default())
}

fn is_local(config: &TutorConfig) -> bool {
    let b = config.effective_base_url();
    ["http://localhost", "http://127.0.0.1", "http://[::1]"]
        .iter()
        .any(|p| b.starts_with(p))
}

#[derive(Serialize)]
pub struct TutorStatus {
    /// A map can be requested.
    pub ready: bool,
    pub has_key: bool,
    /// Last four characters of the saved key.
    pub key_hint: Option<String>,
    pub provider: Provider,
    pub model: String,
    pub base_url: String,
    pub default_model: &'static str,
}

fn status_inner(state: &AppState) -> CmdResult<TutorStatus> {
    let c = config(state)?;
    let key_hint = state.lib.setting(KEY_HINT_KEY)?;
    let has_key = key_hint.is_some();
    Ok(TutorStatus {
        ready: c.validate().is_ok() && (has_key || is_local(&c)),
        has_key,
        key_hint,
        provider: c.provider,
        model: c.model,
        base_url: c.base_url,
        default_model: bc_tutor::DEFAULT_ANTHROPIC_MODEL,
    })
}

#[tauri::command]
pub fn tutor_status(state: State<'_, AppState>) -> CmdResult<TutorStatus> {
    status_inner(&state)
}

/// Save provider settings. `api_key: None` keeps the stored key.
#[tauri::command]
pub fn tutor_save(
    state: State<'_, AppState>,
    config: TutorConfig,
    api_key: Option<String>,
) -> CmdResult<TutorStatus> {
    config.validate()?;
    if let Some(key) = api_key.map(|k| k.trim().to_string()) {
        if key.is_empty() {
            state.tutor_store.clear()?;
            state.lib.delete_setting(KEY_HINT_KEY)?;
        } else {
            if key.len() > 512 || key.chars().any(char::is_whitespace) {
                return Err(CmdError::invalid(
                    "That doesn't look like an API key (it should be one line with no spaces).",
                ));
            }
            state.tutor_store.save(&key)?;
            let hint: String = key
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            state.lib.set_setting(KEY_HINT_KEY, &hint)?;
        }
    }
    state.lib.set_setting_json(CONFIG_KEY, &config)?;
    status_inner(&state)
}

/// Remove the key and provider settings.
#[tauri::command]
pub fn tutor_forget(state: State<'_, AppState>) -> CmdResult<TutorStatus> {
    state.tutor_store.clear()?;
    state.lib.delete_setting(KEY_HINT_KEY)?;
    state.lib.delete_setting(CONFIG_KEY)?;
    status_inner(&state)
}

fn tutor(state: &AppState) -> CmdResult<Tutor> {
    let c = config(state)?;
    c.validate()?;
    let key = state.tutor_store.load()?.unwrap_or_default();
    if key.is_empty() && !is_local(&c) {
        return Err(CmdError::new(
            "not_configured",
            "Add an API key in Settings → Learning assistant first.",
        ));
    }
    Ok(Tutor::new(state.http.clone(), c, key))
}

/// Send a tiny request to check the key, model and endpoint.
#[tauri::command]
pub async fn tutor_test(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(tutor(&state)?.ping().await?)
}

#[derive(Serialize)]
pub struct ZoneOverview {
    pub profile: ZoneProfile,
    pub maps: Vec<ZoneMap>,
}

#[tauri::command]
pub fn zone_overview(state: State<'_, AppState>, id: i64) -> CmdResult<ZoneOverview> {
    Ok(ZoneOverview {
        profile: state.lib.zone_profile(id)?,
        maps: state.lib.zone_maps(id, 50)?,
    })
}

#[tauri::command]
pub fn zone_set_profile(
    state: State<'_, AppState>,
    id: i64,
    background: String,
    goal: String,
) -> CmdResult<ZoneProfile> {
    let clip = |s: &str| bc_tutor::clip(s, 2000).to_string();
    Ok(state
        .lib
        .set_zone_profile(id, &clip(&background), &clip(&goal), None)?)
}

#[tauri::command]
pub fn zone_feedback(
    state: State<'_, AppState>,
    map_id: String,
    feedback: ZoneFeedback,
) -> CmdResult<ZoneProfile> {
    Ok(state.lib.set_zone_feedback(&map_id, feedback)?)
}

#[tauri::command]
pub fn zone_delete(state: State<'_, AppState>, map_id: String) -> CmdResult<()> {
    Ok(state.lib.delete_zone_map(&map_id)?)
}

/// Text of a PDF page, one line per text segment row.
fn pdf_page_text(state: &AppState, path: &std::path::Path, page: u32) -> String {
    let Ok(runs) = state.pdf().and_then(|p| Ok(p.text_runs(path, page)?)) else {
        return String::new();
    };
    let mut out = String::new();
    let mut last_y: Option<f32> = None;
    for r in runs {
        if let Some(y) = last_y {
            out.push(if (r.y - y).abs() > r.h.max(0.004) * 0.5 {
                '\n'
            } else {
                ' '
            });
        }
        out.push_str(r.text.trim());
        last_y = Some(r.y);
    }
    out
}

fn squash(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Map the zone for where the reader is. EPUB passages come from the UI
/// (it knows exactly what is on screen); PDF text is read here.
#[tauri::command]
pub async fn zone_generate(
    app: AppHandle,
    id: i64,
    locator: Locator,
    label: String,
    passage: Option<String>,
) -> CmdResult<ZoneMap> {
    let app2 = app.clone();
    let loc2 = locator.clone();
    let (req, tutor) = crate::commands::blocking(move || {
        let state = app2.state::<AppState>();
        let tutor = tutor(&state)?;
        let src = state.lib.book_source(id)?;
        let summary = state.lib.book_summary(id)?;
        let text = match (&loc2, src.format) {
            (Locator::Pdf { page, .. }, BookFormat::Pdf) => {
                let path = crate::commands::opened_path(&state, id)?;
                let count = summary
                    .page_count
                    .and_then(|c| u32::try_from(c).ok())
                    .unwrap_or(page + 1);
                let last = count.saturating_sub(1);
                let pages = page.saturating_sub(1)..=(*page + 1).min(last);
                pages
                    .map(|p| (p, pdf_page_text(&state, &path, p)))
                    .filter(|(_, t)| !t.trim().is_empty())
                    .map(|(p, t)| format!("[Page {}]\n{t}", p + 1))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            }
            _ => passage.unwrap_or_default(),
        };
        if text.trim().is_empty() {
            return Err(CmdError::new(
                "zone_empty",
                if src.format == BookFormat::Pdf {
                    "This page has no selectable text (it may be a scan), so there's nothing to map."
                } else {
                    "There's no text on this page to map. Turn to a page with some reading on it."
                },
            ));
        }
        let profile = state.lib.zone_profile(id)?;
        let earlier = state
            .lib
            .zone_maps(id, 6)?
            .into_iter()
            .map(|m| EarlierZone {
                label: m.label,
                concepts: m.guide["zone"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|z| z["concept"].as_str().map(str::to_string))
                    .collect(),
                feedback: m.feedback.map(|f| f.as_str().to_string()),
            })
            .collect();
        let hay = squash(&text);
        let marks = state
            .lib
            .annotations(id)?
            .into_iter()
            .filter_map(|a| {
                let t = a.text?;
                let needle = squash(bc_tutor::clip(&t, 120));
                (!needle.is_empty() && hay.contains(&needle)).then_some(ReaderMark { text: t, note: a.note })
            })
            .take(10)
            .collect();
        let req = ZoneRequest {
            title: summary.title,
            author: summary.author,
            location: if label.trim().is_empty() {
                let pct = match &loc2 {
                    Locator::Epub { percent, .. } => *percent,
                    Locator::Pdf { .. } => 0.0,
                };
                loc2.label(pct)
            } else {
                label
            },
            passage: text,
            learner: Learner {
                background: profile.background,
                goal: profile.goal,
                stretch: profile.stretch,
            },
            earlier,
            marks,
        };
        Ok((req, tutor))
    })
    .await?;
    let out = tutor.generate(&req).await?;
    let guide = serde_json::to_value(&out.guide).map_err(|e| CmdError::internal(e.to_string()))?;
    let state = app.state::<AppState>();
    Ok(state
        .lib
        .add_zone_map(id, &locator, &req.location, &guide, &out.model)?)
}
