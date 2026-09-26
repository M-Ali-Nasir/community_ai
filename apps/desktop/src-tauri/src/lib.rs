//! Tauri host. Networking is `community-app` / `MeshSwarm`, not the WebView.
//! Do not wire production chat to TypeScript inferenceEngine or WebRTC.

use std::sync::Arc;

use community_app::{AppOptions, CommunityApp};
use tauri::State;
use tokio::sync::Mutex;

struct AppState {
    session: Mutex<Option<Arc<CommunityApp>>>,
}

async fn require_app(state: &State<'_, AppState>) -> Result<Arc<CommunityApp>, String> {
    state
        .session
        .lock()
        .await
        .as_ref()
        .cloned()
        .ok_or_else(|| "mesh not started".into())
}

#[tauri::command]
async fn session(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    match state.session.lock().await.as_ref() {
        Some(app) => serde_json::to_value(app.session_view()).map_err(|e| e.to_string()),
        None => Ok(serde_json::json!({
            "started": false,
            "local_peer_id": null,
            "model_id": "",
            "wan_status": "PHYSICAL WAN VERIFIED — NOT TESTED"
        })),
    }
}

#[tauri::command]
async fn peer_id(state: State<'_, AppState>) -> Result<String, String> {
    let app = require_app(&state).await?;
    Ok(app.peer_id().to_string())
}

#[tauri::command]
async fn ready_peers(state: State<'_, AppState>) -> Result<usize, String> {
    match state.session.lock().await.as_ref() {
        Some(app) => Ok(app.swarm.ready_count().await),
        None => Ok(0),
    }
}

#[tauri::command]
async fn peers(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let app = require_app(&state).await?;
    serde_json::to_value(app.peers_view().await).map_err(|e| e.to_string())
}

#[tauri::command]
async fn network(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let app = require_app(&state).await?;
    serde_json::to_value(app.network_view().await).map_err(|e| e.to_string())
}

#[tauri::command]
async fn models(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let app = require_app(&state).await?;
    serde_json::to_value(app.models_view().await).map_err(|e| e.to_string())
}

#[tauri::command]
async fn tasks(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let app = require_app(&state).await?;
    serde_json::to_value(app.tasks_view().await).map_err(|e| e.to_string())
}

#[tauri::command]
async fn dial(state: State<'_, AppState>, addr: String) -> Result<String, String> {
    let app = require_app(&state).await?;
    app.dial_peer(&addr).await.map_err(|e| e.to_string())
}

/// Real mesh inference. Never returns template / synthetic chat.
#[tauri::command]
async fn chat(state: State<'_, AppState>, prompt: String) -> Result<serde_json::Value, String> {
    let app = require_app(&state).await?;
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("prompt is empty".into());
    }
    let result = app.chat(prompt).await.map_err(|e| e.to_string())?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            session: Mutex::new(None),
        })
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut opts = AppOptions::desktop_defaults();
                // Default: listen on ephemeral; mDNS/STUN optional (not WAN proof).
                opts.bind = "0.0.0.0:50051".parse().unwrap();
                match CommunityApp::start(opts).await {
                    Ok(session) => {
                        let st: State<'_, AppState> = handle.state();
                        *st.session.lock().await = Some(Arc::new(session));
                    }
                    Err(e) => {
                        eprintln!("mesh start failed: {e}");
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            session, peer_id, ready_peers, peers, network, models, tasks, dial, chat
        ])
        .run(tauri::generate_context!())
        .expect("tauri");
}
