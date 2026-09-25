//! Tauri host. Networking is `community-app` / `MeshSwarm`, not the WebView.

use std::sync::Arc;

use community_app::{AppOptions, CommunityApp};
use tauri::State;
use tokio::sync::Mutex;

struct AppState {
    session: Mutex<Option<Arc<CommunityApp>>>,
}

#[tauri::command]
async fn peer_id(state: State<'_, AppState>) -> Result<String, String> {
    let g = state.session.lock().await;
    match g.as_ref() {
        Some(app) => Ok(app.peer_id().to_string()),
        None => Err("mesh not started".into()),
    }
}

#[tauri::command]
async fn ready_peers(state: State<'_, AppState>) -> Result<usize, String> {
    let g = state.session.lock().await;
    match g.as_ref() {
        Some(app) => Ok(app.swarm.ready_count().await),
        None => Ok(0),
    }
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
                opts.enable_mdns = false;
                opts.bind = "0.0.0.0:50051".parse().unwrap();
                match CommunityApp::start(opts).await {
                    Ok(session) => {
                        let st: State<AppState> = handle.state();
                        *st.session.lock().await = Some(Arc::new(session));
                    }
                    Err(e) => {
                        eprintln!("mesh start failed: {e}");
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![peer_id, ready_peers])
        .run(tauri::generate_context!())
        .expect("tauri");
}
