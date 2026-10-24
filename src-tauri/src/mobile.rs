//! Android platform services. Native token grants stay with Google Play services;
//! document URIs are copied into private storage before Rust reads them.
use bc_drive::{Auth, DriveError};
use serde::Deserialize;
use serde_json::json;
use tauri::{plugin::PluginHandle, AppHandle, Manager};

use crate::error::{CmdError, CmdResult};

pub struct AndroidBridge(PluginHandle<tauri::Wry>);

pub fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("android-bridge")
        .setup(|app, api| {
            let handle = api.register_android_plugin("app.bibliochad.reader", "AndroidBridge")?;
            app.manage(AndroidBridge(handle));
            Ok(())
        })
        .build()
}

#[derive(Deserialize)]
struct Token {
    token: String,
}

async fn token(app: &AppHandle, interactive: bool, invalidate: bool) -> bc_drive::Result<String> {
    app.state::<AndroidBridge>()
        .0
        .run_mobile_plugin_async::<Token>(
            "authorize",
            json!({ "interactive": interactive, "invalidate": invalidate }),
        )
        .await
        .map(|response| response.token)
        .map_err(|e| {
            if matches!(&e, tauri::plugin::mobile::PluginInvokeError::InvokeRejected(reason) if reason.code.as_deref() == Some("reauth")) {
                DriveError::ReauthRequired
            } else if interactive {
                DriveError::AuthFailed(e.to_string())
            } else {
                // A transient native/network failure must not silently sign the
                // user out or remove an otherwise valid Drive session.
                DriveError::Api { status: 503, message: e.to_string() }
            }
        })
}

pub async fn drive_auth(app: &AppHandle, interactive: bool) -> bc_drive::Result<Auth> {
    let initial = token(app, interactive, false).await?;
    let app = app.clone();
    Ok(Auth::with_provider(initial, move |invalidate| {
        let app = app.clone();
        Box::pin(async move { token(&app, false, invalidate).await })
    }))
}

pub async fn revoke(app: &AppHandle) -> CmdResult<()> {
    app.state::<AndroidBridge>()
        .0
        .run_mobile_plugin_async::<serde_json::Value>("revoke", json!({}))
        .await
        .map(|_| ())
        .map_err(|e| CmdError::internal(e.to_string()))
}

#[derive(Deserialize)]
struct Imported {
    path: String,
}

pub fn import_document(app: &AppHandle, uri: &str) -> CmdResult<std::path::PathBuf> {
    app.state::<AndroidBridge>()
        .0
        .run_mobile_plugin::<Imported>("importDocument", json!({"uri": uri}))
        .map(|r| r.path.into())
        .map_err(|e| CmdError::internal(e.to_string()))
}

pub fn write_document(app: &AppHandle, uri: &str, text: &str) -> CmdResult<()> {
    app.state::<AndroidBridge>()
        .0
        .run_mobile_plugin::<serde_json::Value>("writeDocument", json!({"uri": uri, "text": text}))
        .map(|_| ())
        .map_err(|e| CmdError::internal(e.to_string()))
}

pub async fn background(app: &AppHandle) -> CmdResult<()> {
    app.state::<AndroidBridge>()
        .0
        .run_mobile_plugin_async::<serde_json::Value>("backgroundApp", json!({}))
        .await
        .map(|_| ())
        .map_err(|e| CmdError::internal(e.to_string()))
}

pub async fn refresh_viewport(app: &AppHandle) -> CmdResult<()> {
    app.state::<AndroidBridge>()
        .0
        .run_mobile_plugin_async::<serde_json::Value>("refreshViewport", json!({}))
        .await
        .map(|_| ())
        .map_err(|e| CmdError::internal(e.to_string()))
}
