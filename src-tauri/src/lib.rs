mod agent;
mod ai;

use agent::state::AgentState;
use ai::config::AiConfigState;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tauri::Manager;
use tauri::State;

/// Managed state holding the embedded server's actual port.
pub struct ServerPort(pub u16);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let work_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            app.manage(AgentState {
                work_dir: std::sync::Mutex::new(work_dir),
                watcher_task: std::sync::Mutex::new(None),
                app_handle: app.handle().clone(),
            });
            app.manage(AiConfigState::default());

            let app_data = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("data"));
            app.manage(ai::session::ChatSessionStore::new(app_data.clone()));
            app.manage(ai::permissions::PermissionStore::new(app_data.clone()));

            // ── Embedded server startup ──
            let handle = app.handle().clone();
            let data_dir = app_data;

            let port = tauri::async_runtime::block_on(async { start_server(data_dir).await })
                .unwrap_or_else(|e| {
                    tracing::warn!("Embedded server failed to start: {e:#}");
                    0
                });

            if port > 0 {
                tracing::info!("Embedded server listening on port {}", port);
                let _ = handle.emit("server-ready", port);
            }

            app.manage(ServerPort(port));
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            agent::commands::list_files,
            agent::commands::read_file,
            agent::commands::read_file_base64,
            agent::commands::write_file,
            agent::commands::create_file,
            agent::commands::change_work_dir,
            agent::commands::open_folder,
            ai::chat::set_ai_config,
            ai::chat::get_ai_config_status,
            ai::chat::set_ai_model,
            ai::chat::ai_chat,
            ai::research::research_search_native,
            ai::research::research_extract_and_save,
            ai::session::list_sessions,
            ai::session::load_session,
            ai::session::delete_session,
            ai::permissions::get_permission_config,
            ai::permissions::resolve_permission_request,
            ai::permissions::set_permission_config,
            ai::search::ai_search,
            get_server_port,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn start_server(data_dir: PathBuf) -> anyhow::Result<u16> {
    use modeler_server::{AppState, Config};

    std::fs::create_dir_all(&data_dir)?;

    let db_path = data_dir.join("modeler.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());

    let cfg = Config::new(
        database_url,
        "tauri-dev-secret".to_string(),
        data_dir.to_string_lossy().to_string(),
        0,
    );

    let pool = modeler_server::db::init_pool(&cfg.database_url).await;

    let state = AppState {
        pool,
        config: cfg,
        room_registry: Arc::new(modeler_server::sync::room::RoomRegistry::new()),
    };

    modeler_server::serve(state).await
}

#[tauri::command]
fn get_server_port(state: State<'_, ServerPort>) -> u16 {
    state.0
}
