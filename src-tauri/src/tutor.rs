//! Learning assistant commands: provider settings, per-book learner
//! profiles, and Zone of Proximal Development maps.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use bc_core::{BookFormat, Locator};
use bc_drive::TokenStore;
use bc_library::{
    lab_fingerprint, BookChatMessage, LabChatMessage, LabGuideRecord, Library, ZoneFeedback,
    ZoneMap, ZoneProfile,
};
use bc_tutor::{
    ChatMessage, ChatRequest, EarlierZone, LabRequest, Learner, Provider, ReaderMark, Tutor,
    TutorConfig, ZoneRequest,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::{CmdError, CmdResult};
use crate::state::{AppState, ChatRun};

const CONFIG_KEY: &str = "tutor_config";
const KEY_HINT_KEY: &str = "tutor_key_hint";

/// Where the API key is kept: the OS credential store.
pub fn key_store(_lib: &Arc<Library>) -> Arc<dyn TokenStore> {
    Arc::new(bc_drive::KeyringStore::new("BiblioChad", "llm-api-key"))
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

// ------------------------------------------------------------------ labs

#[derive(Serialize)]
pub struct LabOverview {
    pub lab: Option<LabGuideRecord>,
    /// The saved guide was built from a different version of the book file.
    pub stale: bool,
    /// A one-step Undo revision from an applied co-pilot edit exists.
    pub has_undo: bool,
}

fn current_fingerprint(state: &AppState, id: i64) -> CmdResult<String> {
    let src = state.lib.book_source(id)?;
    Ok(lab_fingerprint(&src.key, src.md5.as_deref()))
}

#[tauri::command]
pub fn lab_overview(state: State<'_, AppState>, id: i64) -> CmdResult<LabOverview> {
    let lab = state.lib.lab_guide(id)?;
    let stale = match &lab {
        Some(l) => l.fingerprint != current_fingerprint(&state, id)?,
        None => false,
    };
    let has_undo = state.lib.has_lab_revision(id)?;
    Ok(LabOverview {
        lab,
        stale,
        has_undo,
    })
}

#[tauri::command]
pub fn lab_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(state.lib.delete_lab_guide(id)?)
}

/// Export the saved labs as a standalone Markdown artefact (Mermaid
/// diagrams included as fenced blocks).
#[tauri::command]
pub fn lab_export(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    dest: String,
) -> CmdResult<()> {
    let record = state
        .lib
        .lab_guide(id)?
        .ok_or_else(|| CmdError::new("not_found", "Build the labs first."))?;
    let summary = state.lib.book_summary(id)?;
    let guide: bc_tutor::LabGuide =
        serde_json::from_value(record.guide).map_err(|e| CmdError::internal(e.to_string()))?;
    let md = guide.to_markdown(&summary.title, summary.author.as_deref());
    let _ = app;
    std::fs::write(dest, md)?;
    Ok(())
}

fn flatten_outline(items: &[bc_reader::pdf::OutlineItem], depth: usize, out: &mut Vec<String>) {
    for item in items {
        if !item.title.trim().is_empty() {
            out.push(format!("{}{}", "  ".repeat(depth), item.title.trim()));
        }
        flatten_outline(&item.children, depth + 1, out);
    }
}

/// Whole-book plain text + contents for lab building. EPUB text comes from
/// the spine documents; PDF text from the selectable text layer.
fn extract_book_text(
    state: &AppState,
    id: i64,
    path: &std::path::Path,
    format: BookFormat,
) -> CmdResult<(String, String, Option<u32>)> {
    match format {
        BookFormat::Epub => {
            let archive = crate::books::epub_archive(state, id, path)?;
            let mut a = archive.lock().unwrap();
            let sections = a.extract_sections()?;
            if sections.is_empty() {
                return Err(CmdError::new(
                    "lab_empty",
                    "This book has no readable text to build labs from.",
                ));
            }
            let toc = sections
                .iter()
                .map(|s| {
                    let first = s.text.lines().next().unwrap_or("").trim();
                    if first.len() > 3 && first.chars().count() <= 120 {
                        format!("{} — {}", s.label, first)
                    } else {
                        s.label.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let full = sections
                .iter()
                .map(|s| format!("[{}]\n{}", s.label, s.text))
                .collect::<Vec<_>>()
                .join("\n\n");
            Ok((full, toc, None))
        }
        BookFormat::Pdf => {
            let pdf = state.pdf()?;
            let info = pdf.info(path)?;
            let mut toc_lines = Vec::new();
            if let Ok(outline) = pdf.outline(path) {
                flatten_outline(&outline, 0, &mut toc_lines);
            }
            // Scanned books can be huge; 2000 pages is far beyond any real
            // lab source and keeps a pathological file from running for ages.
            let pages = info.page_count.min(2000);
            let mut parts = Vec::new();
            for p in 0..pages {
                let t = pdf_page_text(state, path, p);
                if !t.trim().is_empty() {
                    parts.push(format!("[Page {}]\n{t}", p + 1));
                }
            }
            if parts.is_empty() {
                return Err(CmdError::new(
                    "lab_empty",
                    "This PDF has no selectable text (it may be a scan), so there is nothing to build labs from.",
                ));
            }
            Ok((
                parts.join("\n\n"),
                toc_lines.join("\n"),
                Some(info.page_count),
            ))
        }
    }
}

/// Analyse the whole book and build its lab series. The guide is saved
/// with the book and returned again by `lab_overview` on every later open.
#[tauri::command]
pub async fn lab_generate(
    app: AppHandle,
    id: i64,
    lab_count: Option<u32>,
    focus: Option<String>,
) -> CmdResult<LabGuideRecord> {
    let path = crate::books::ensure_local(&app, id).await?;
    let app2 = app.clone();
    let (req, tutor, fingerprint) = crate::commands::blocking(move || {
        let state = app2.state::<AppState>();
        let tutor = tutor(&state)?;
        let src = state.lib.book_source(id)?;
        let summary = state.lib.book_summary(id)?;
        let fingerprint = lab_fingerprint(&src.key, src.md5.as_deref());
        let (full, toc, page_count) = extract_book_text(&state, id, &path, src.format)?;
        let sampled = full.chars().count() > bc_tutor::MAX_BOOK_CHARS;
        let excerpt = bc_tutor::sample_book_text(&full, bc_tutor::MAX_BOOK_CHARS);
        let profile = state.lib.zone_profile(id)?;
        let req = LabRequest {
            title: summary.title,
            author: summary.author,
            format: Some(src.format.as_str().to_string()),
            page_count,
            toc,
            excerpt,
            sampled,
            learner: Learner {
                background: profile.background,
                goal: profile.goal,
                stretch: profile.stretch,
            },
            lab_count: lab_count.filter(|n| (1..=10).contains(n)),
            focus: focus
                .map(|f| bc_tutor::clip(&f, 1000).to_string())
                .unwrap_or_default(),
        };
        Ok((req, tutor, fingerprint))
    })
    .await?;
    let out = tutor.generate_labs(&req).await?;
    let guide = serde_json::to_value(&out.guide).map_err(|e| CmdError::internal(e.to_string()))?;
    let state = app.state::<AppState>();
    Ok(state
        .lib
        .save_lab_guide(id, &guide, &out.model, &fingerprint)?)
}

// ------------------------------------------------------------------ chat (BC-2)

/// One streamed chunk, emitted as the `chat-delta` event. Consumers must
/// ignore events whose `run_id` is not the run they started.
#[derive(Serialize, Clone)]
pub struct ChatDelta {
    pub run_id: String,
    pub book_id: i64,
    pub text: String,
}

#[derive(Serialize)]
pub struct ChatReplyOut {
    pub run_id: String,
    /// Full authoritative reply text (a missed event never loses it).
    pub text: String,
    pub model: String,
    /// The provider cut the answer off; `text` is the partial reply.
    pub truncated: bool,
    /// The context mode that was actually used ("passage" | "book").
    pub context_mode: String,
    /// Whole-book context was sampled because the book is long.
    pub context_sampled: bool,
    /// Characters of book context available before sampling/clipping.
    pub context_chars: usize,
}

/// Send one chat turn and stream the reply. The ONLY command that talks
/// to the provider for chat: opening a book or panel sends nothing.
/// `history` ends with the new user message. `context_mode` is
/// "passage" (on-screen text: UI-supplied for EPUB, page±1 read here
/// for PDF) or "book" (whole-book text, sampled in bc-tutor).
/// Deltas arrive as `chat-delta` events; the return value is final.
/// Cancel with `chat_cancel`; starting a new run cancels the old one.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn chat_send(
    app: AppHandle,
    id: i64,
    locator: Locator,
    label: String,
    passage: Option<String>,
    context_mode: String,
    history: Vec<ChatMessage>,
    run_id: String,
    outline: Option<String>,
) -> CmdResult<ChatReplyOut> {
    if run_id.trim().is_empty() {
        return Err(CmdError::invalid("chat_send needs a run_id."));
    }
    if context_mode != "passage" && context_mode != "book" {
        return Err(CmdError::invalid(
            "context_mode must be \"passage\" or \"book\".",
        ));
    }
    let book_path = if context_mode == "book" {
        Some(crate::books::ensure_local(&app, id).await?)
    } else {
        None
    };
    let app2 = app.clone();
    let loc2 = locator.clone();
    let (req, tutor, sampled, context_chars) = crate::commands::blocking(move || {
        let state = app2.state::<AppState>();
        let tutor = tutor(&state)?;
        let src = state.lib.book_source(id)?;
        let summary = state.lib.book_summary(id)?;
        let mut outline = outline.unwrap_or_default();
        let (context, sampled) = if let Some(path) = &book_path {
            let (full, toc, _pages) = extract_book_text(&state, id, path, src.format)
                .map_err(|e| {
                    if e.kind == "lab_empty" {
                        CmdError::new(
                            "chat_no_text",
                            "This book has no readable text (it may be a scan), so whole-book chat isn't available.",
                        )
                    } else {
                        e
                    }
                })?;
            if full.trim().is_empty() {
                return Err(CmdError::new(
                    "chat_no_text",
                    "This book has no readable text (it may be a scan), so whole-book chat isn't available.",
                ));
            }
            if outline.trim().is_empty() {
                outline = toc;
            }
            let sampled = full.chars().count() > bc_tutor::MAX_BOOK_CHARS;
            (full, sampled)
        } else {
            let text = match (&loc2, src.format) {
                (Locator::Pdf { page, .. }, BookFormat::Pdf) => {
                    let path = crate::commands::opened_path(&state, id)?;
                    let count = summary
                        .page_count
                        .and_then(|c| u32::try_from(c).ok())
                        .unwrap_or(page + 1);
                    let last = count.saturating_sub(1);
                    (page.saturating_sub(1)..=(*page + 1).min(last))
                        .map(|p| (p, pdf_page_text(&state, &path, p)))
                        .filter(|(_, t)| !t.trim().is_empty())
                        .map(|(p, t)| format!("[Page {}]\n{t}", p + 1))
                        .collect::<Vec<_>>()
                        .join("\n\n")
                }
                _ => passage.unwrap_or_default(),
            };
            (text, false)
        };
        let profile = state.lib.zone_profile(id)?;
        let context_chars = context.chars().count();
        let req = ChatRequest {
            title: summary.title,
            author: summary.author,
            outline,
            location: if label.trim().is_empty() {
                let pct = match &loc2 {
                    Locator::Epub { percent, .. } => *percent,
                    Locator::Pdf { .. } => 0.0,
                };
                loc2.label(pct)
            } else {
                label
            },
            context,
            context_sampled: sampled,
            learner: Learner {
                background: profile.background,
                goal: profile.goal,
                stretch: profile.stretch,
            },
            history,
            system_override: None,
        };
        Ok((req, tutor, sampled, context_chars))
    })
    .await?;
    run_chat_stream(
        app,
        id,
        run_id,
        tutor,
        req,
        context_mode,
        sampled,
        context_chars,
    )
    .await
}

/// Shared streaming tail for book chat and lab chat: registers the run
/// (cancelling any previous one), streams coalesced `chat-delta`
/// events, and resolves with the authoritative full reply.
#[allow(clippy::too_many_arguments)]
async fn run_chat_stream(
    app: AppHandle,
    id: i64,
    run_id: String,
    tutor: Tutor,
    req: ChatRequest,
    context_mode: String,
    sampled: bool,
    context_chars: usize,
) -> CmdResult<ChatReplyOut> {
    // Register this run, cancelling any previous one.
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<AppState>();
        let mut slot = state.chat_run.lock().unwrap();
        if let Some(old) = slot.take() {
            old.cancel();
        }
        *slot = Some(ChatRun {
            run_id: run_id.clone(),
            cancelled: cancelled.clone(),
            abort: None,
        });
    }

    let pending: Arc<StdMutex<(String, Instant)>> =
        Arc::new(StdMutex::new((String::new(), Instant::now())));
    let emit_pending = {
        let app = app.clone();
        let pending = pending.clone();
        let run_id = run_id.clone();
        move || {
            let mut p = pending.lock().unwrap();
            if !p.0.is_empty() {
                let text = std::mem::take(&mut p.0);
                p.1 = Instant::now();
                let _ = app.emit(
                    "chat-delta",
                    ChatDelta {
                        run_id: run_id.clone(),
                        book_id: id,
                        text,
                    },
                );
            }
        }
    };
    let flush = emit_pending.clone();
    let cancelled2 = cancelled.clone();
    let task = tokio::spawn(async move {
        let result = tutor
            .chat_stream(&req, |delta| {
                if cancelled2.load(Ordering::SeqCst) {
                    return false;
                }
                // Coalesce: emit at most every 80ms or 64 chars.
                let due = {
                    let mut p = pending.lock().unwrap();
                    p.0.push_str(delta);
                    p.0.len() >= 64 || p.1.elapsed() >= Duration::from_millis(80)
                };
                if due {
                    emit_pending();
                }
                true
            })
            .await;
        flush();
        result
    });
    {
        let state = app.state::<AppState>();
        let mut slot = state.chat_run.lock().unwrap();
        if let Some(run) = slot.as_mut().filter(|r| r.run_id == run_id) {
            run.abort = Some(task.abort_handle());
        }
    }

    let joined = task.await;
    {
        let state = app.state::<AppState>();
        let mut slot = state.chat_run.lock().unwrap();
        if slot.as_ref().map(|r| r.run_id.as_str()) == Some(run_id.as_str()) {
            *slot = None;
        }
    }
    match joined {
        Ok(Ok(reply)) => Ok(ChatReplyOut {
            run_id,
            text: reply.text,
            model: reply.model,
            truncated: reply.truncated,
            context_mode,
            context_sampled: sampled,
            context_chars,
        }),
        Ok(Err(e)) => Err(e.into()),
        Err(join) if join.is_cancelled() || cancelled.load(Ordering::SeqCst) => {
            Err(CmdError::new("cancelled", "Cancelled."))
        }
        Err(join) => Err(CmdError::internal(join.to_string())),
    }
}

/// Cancel the in-flight chat run with this `run_id` (no-op if it already
/// finished). The pending `chat_send` resolves with kind "cancelled";
/// text already delivered via `chat-delta` events stays with the UI.
#[tauri::command]
pub fn chat_cancel(state: State<'_, AppState>, run_id: String) -> CmdResult<()> {
    let slot = state.chat_run.lock().unwrap();
    if let Some(run) = slot.as_ref().filter(|r| r.run_id == run_id) {
        run.cancel();
    }
    Ok(())
}

// ------------------------------------------------------------- chat history (BC-4)
// Local-only persistence: rows live in bc-library's book_chat_messages
// and are never part of the Drive sync file. Explain (BC-3) does not
// write here; only the book chat panel appends.

/// A book's saved chat, oldest first (most recent 500 messages).
#[tauri::command]
pub fn chat_history(state: State<'_, AppState>, id: i64) -> CmdResult<Vec<BookChatMessage>> {
    Ok(state.lib.book_chat_messages(id, 500)?)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn chat_history_append(
    state: State<'_, AppState>,
    id: i64,
    role: String,
    content: String,
    locator: Option<Locator>,
    label: String,
    model: String,
    context_mode: String,
) -> CmdResult<BookChatMessage> {
    Ok(state.lib.add_book_chat_message(
        id,
        &role,
        &content,
        locator.as_ref(),
        &label,
        &model,
        &context_mode,
    )?)
}

/// Delete a book's saved chat (other books untouched).
#[tauri::command]
pub fn chat_history_clear(state: State<'_, AppState>, id: i64) -> CmdResult<usize> {
    Ok(state.lib.clear_book_chat(id)?)
}

#[derive(Serialize)]
pub struct ChatBookStatus {
    /// Whole-book chat can be used for this book.
    pub available: bool,
    /// The book is long, so whole-book context will be sampled.
    pub sampled: bool,
    /// Characters of readable book text found.
    pub chars: usize,
    /// Why whole-book chat is unavailable, when it is.
    pub reason: Option<String>,
}

/// Can this book be chatted about as a whole? Extracts the book text on
/// this device only (nothing is sent anywhere); the UI calls this when
/// the reader picks whole-book mode so scanned/no-text books disable
/// that mode with an explanation instead of failing at send time.
#[tauri::command]
pub async fn chat_book_status(app: AppHandle, id: i64) -> CmdResult<ChatBookStatus> {
    let path = crate::books::ensure_local(&app, id).await?;
    crate::commands::blocking(move || {
        let state = app.state::<AppState>();
        let src = state.lib.book_source(id)?;
        match extract_book_text(&state, id, &path, src.format) {
            Ok((full, _toc, _pages)) if !full.trim().is_empty() => Ok(ChatBookStatus {
                available: true,
                sampled: full.chars().count() > bc_tutor::MAX_BOOK_CHARS,
                chars: full.chars().count(),
                reason: None,
            }),
            Ok(_) => Ok(ChatBookStatus {
                available: false,
                sampled: false,
                chars: 0,
                reason: Some(
                    "This book has no readable text (it may be a scan), so whole-book chat isn't available."
                        .into(),
                ),
            }),
            Err(e) if e.kind == "lab_empty" => Ok(ChatBookStatus {
                available: false,
                sampled: false,
                chars: 0,
                reason: Some(
                    "This book has no readable text (it may be a scan), so whole-book chat isn't available."
                        .into(),
                ),
            }),
            Err(e) => Err(e),
        }
    })
    .await
}

// ------------------------------------------------------------- lab co-pilot (BC-5)
// Lab-scoped chat: clarify/troubleshoot by default (never mutates the
// saved guide); edits arrive as structured proposals in the reply and
// are persisted only by `lab_proposal_apply` on reader confirmation.

/// Send one lab-chat turn with the chosen lab/step pre-attached (steps,
/// commands, Mermaid source, checks, ticked state) plus book metadata,
/// outline and bounded sampled excerpts. Streams `chat-delta` like
/// book chat; cancel with `chat_cancel`. Nothing is sent until called.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn lab_chat_send(
    app: AppHandle,
    id: i64,
    lab_index: usize,
    step_index: Option<usize>,
    history: Vec<ChatMessage>,
    run_id: String,
    ticked: Vec<String>,
    outline: Option<String>,
) -> CmdResult<ChatReplyOut> {
    if run_id.trim().is_empty() {
        return Err(CmdError::invalid("lab_chat_send needs a run_id."));
    }
    let path = crate::books::ensure_local(&app, id).await.ok();
    let app2 = app.clone();
    let (req, tutor, context_chars) = crate::commands::blocking(move || {
        let state = app2.state::<AppState>();
        let tutor = tutor(&state)?;
        let src = state.lib.book_source(id)?;
        let summary = state.lib.book_summary(id)?;
        let record = state
            .lib
            .lab_guide(id)?
            .ok_or_else(|| CmdError::new("lab_missing", "Build the labs for this book first."))?;
        let guide: bc_tutor::LabGuide =
            serde_json::from_value(record.guide).map_err(|e| CmdError::internal(e.to_string()))?;
        let profile = state.lib.zone_profile(id)?;
        let learner = Learner {
            background: profile.background,
            goal: profile.goal,
            stretch: profile.stretch,
        };
        // Book excerpts are a bonus: a scanned/unreadable book still gets
        // lab help from the saved guide alone.
        let (excerpt, toc) = match &path {
            Some(p) => match extract_book_text(&state, id, p, src.format) {
                Ok((full, toc, _)) => (
                    bc_tutor::sample_book_text(&full, bc_tutor::LAB_CHAT_EXCERPT_CHARS),
                    toc,
                ),
                Err(_) => (String::new(), String::new()),
            },
            None => (String::new(), String::new()),
        };
        let outline = {
            let o = outline.unwrap_or_default();
            if o.trim().is_empty() {
                toc
            } else {
                o
            }
        };
        let context = bc_tutor::build_lab_context(
            &guide, lab_index, step_index, &ticked, &outline, &excerpt, &learner,
        )
        .map_err(CmdError::from)?;
        let location = match (guide.labs.get(lab_index), step_index) {
            (Some(lab), Some(si)) => {
                format!("Lab {} ({}) · Step {}", lab_index + 1, lab.title, si + 1)
            }
            (Some(lab), None) => format!("Lab {} ({})", lab_index + 1, lab.title),
            _ => format!("Lab {}", lab_index + 1),
        };
        let context_chars = context.chars().count();
        let req = ChatRequest {
            title: summary.title,
            author: summary.author,
            outline,
            location,
            context,
            context_sampled: false,
            learner,
            history,
            system_override: Some(bc_tutor::lab_chat_system_prompt()),
        };
        Ok((req, tutor, context_chars))
    })
    .await?;
    run_chat_stream(
        app,
        id,
        run_id,
        tutor,
        req,
        "lab".into(),
        false,
        context_chars,
    )
    .await
}

/// A book's saved lab chat, oldest first (local-only; separate from book chat).
#[tauri::command]
pub fn lab_chat_history(state: State<'_, AppState>, id: i64) -> CmdResult<Vec<LabChatMessage>> {
    Ok(state.lib.lab_chat_messages(id, 500)?)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn lab_chat_history_append(
    state: State<'_, AppState>,
    id: i64,
    role: String,
    content: String,
    lab_index: i64,
    step_index: Option<i64>,
    label: String,
    model: String,
) -> CmdResult<LabChatMessage> {
    Ok(state
        .lib
        .add_lab_chat_message(id, &role, &content, lab_index, step_index, &label, &model)?)
}

/// Delete a book's lab chat only; its book chat is untouched.
#[tauri::command]
pub fn lab_chat_history_clear(state: State<'_, AppState>, id: i64) -> CmdResult<usize> {
    Ok(state.lib.clear_lab_chat(id)?)
}

/// Apply a reader-confirmed lab-edit proposal. Re-validates against the
/// *current* saved guide and refuses atomically (nothing written) when
/// invalid; on success the previous guide becomes the Undo revision.
#[tauri::command]
pub fn lab_proposal_apply(
    state: State<'_, AppState>,
    id: i64,
    proposal: serde_json::Value,
) -> CmdResult<LabGuideRecord> {
    let proposal: bc_tutor::LabProposal = serde_json::from_value(proposal).map_err(|e| {
        CmdError::new(
            "invalid",
            format!("That lab edit isn't in the expected format, so nothing was changed: {e}"),
        )
    })?;
    let record = state
        .lib
        .lab_guide(id)?
        .ok_or_else(|| CmdError::new("lab_missing", "Build the labs for this book first."))?;
    let guide: bc_tutor::LabGuide =
        serde_json::from_value(record.guide).map_err(|e| CmdError::internal(e.to_string()))?;
    let next = bc_tutor::apply_proposal(&guide, &proposal).map_err(CmdError::from)?;
    let value = serde_json::to_value(&next).map_err(|e| CmdError::internal(e.to_string()))?;
    Ok(state.lib.apply_lab_edit(id, &value)?)
}

/// Undo the most recent applied co-pilot edit (one step). Returns the
/// restored record, or `None` when there is nothing to undo.
#[tauri::command]
pub fn lab_undo(state: State<'_, AppState>, id: i64) -> CmdResult<Option<LabGuideRecord>> {
    Ok(state.lib.undo_lab_edit(id)?)
}
