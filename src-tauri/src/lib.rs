mod agent;
mod ai;

use agent::state::AgentState;
use ai::config::AiConfigState;
use ai::sidecar::SidecarState;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// Managed state holding the embedded server's actual port.
pub struct ServerPort(pub u16);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing so warn/info logs (sidecar startup, server, etc.) are
    // visible in the dev console. RUST_LOG overrides the default level.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    tauri::Builder::default()
        .setup(|app| {
            let work_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            app.manage(AgentState {
                work_dir: std::sync::Mutex::new(work_dir),
                watcher_task: std::sync::Mutex::new(None),
                app_handle: app.handle().clone(),
            });

            let app_data = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("data"));
            app.manage(AiConfigState::new(app_data.clone()));
            app.manage(ai::session::ChatSessionStore::new(app_data.clone()));
            app.manage(ai::permissions::PermissionStore::new(app_data.clone()));
            app.manage(ai::tools::question::QuestionStore::new());
            app.manage(ai::hooks::HookManager::new(app_data.clone()));
            app.manage(ai::skills::SkillRegistry::new(app_data.clone()));
            app.manage(ai::plugins::PluginManager::new(app_data.clone()));
            app.manage(ai::plan::PlanService::new(app_data.clone()));
            let work_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let agent_orchestrator = ai::agent::AgentOrchestrator::new(
                app.handle().clone(),
                work_dir,
            );
            app.manage(agent_orchestrator);
            app.manage(ai::chat::StopFlags::default());
            app.manage(ai::history::OperationHistoryStore::new(app_data.clone()));

            // ── Sidecar state (lazy start on first research search) ──
            let sidecar_dir = app
                .path()
                .resource_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("sidecar");
            let sidecar_dir = if sidecar_dir.join("run.py").exists() {
                sidecar_dir
            } else {
                // Dev fallback: sidecar lives next to src-tauri source
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sidecar")
            };
            let sidecar_state = SidecarState::new(sidecar_dir);

            let config_for_sidecar = app
                .state::<AiConfigState>()
                .get()
                .unwrap_or_default();
            if config_for_sidecar.sidecar_enabled {
                let python = SidecarState::resolve_python_command(
                    config_for_sidecar.sidecar_python_path.as_deref(),
                );
                let sidecar_ref = &sidecar_state;
                let _ = tauri::async_runtime::block_on(async {
                    sidecar_ref.ensure_started(&python).await
                })
                .inspect_err(|e| tracing::warn!("Sidecar unavailable: {e:#}"));
            }
            app.manage(sidecar_state);

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
            agent::commands::compile_latex,
            ai::chat::set_ai_config,
            ai::chat::get_ai_config_status,
            ai::chat::set_ai_model,
            ai::chat::ai_chat,
            ai::chat::stop_generation,
            ai::research::research_search_native,
            ai::research::research_analyze_url,
            ai::research::research_extract_and_save,
            ai::session::list_sessions,
            ai::session::load_session,
            ai::session::delete_session,
            ai::session::rename_session,
            ai::session::archive_session,
            ai::session::unarchive_session,
            ai::session::search_sessions,
            ai::session::export_session,
            ai::history::list_operations,
            ai::history::get_operation_stats,
            ai::permissions::get_permission_config,
            ai::permissions::resolve_permission_request,
            ai::permissions::set_permission_config,
            ai::search::ai_search,
            resolve_question,
            list_tasks,
            list_hooks,
            toggle_hook,
            list_skills,
            list_plugins,
            toggle_plugin,
            get_server_port,
            get_sidecar_status,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                let sidecar = app_handle.state::<SidecarState>();
                tauri::async_runtime::block_on(async { sidecar.stop().await });
            }
        });
}

/// Reports whether the research sidecar is currently running and healthy, so
/// the Settings UI can show academic-search availability.
#[tauri::command]
async fn get_sidecar_status(sidecar: State<'_, SidecarState>) -> Result<bool, String> {
    Ok(sidecar.is_available().await)
}

#[tauri::command]
async fn list_hooks(
    hooks: State<'_, ai::hooks::HookManager>,
) -> Result<Vec<ai::hooks::Hook>, String> {
    Ok(hooks.list_hooks().await)
}

#[tauri::command]
async fn list_tasks(
    conversation_id: String,
    app: AppHandle,
) -> Result<Vec<serde_json::Value>, String> {
    let data_dir = app.path().app_data_dir().unwrap_or_default();
    let path = data_dir.join("task-lists").join(format!("{conversation_id}.json"));
    let tasks: Vec<serde_json::Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default();
    Ok(tasks)
}

#[tauri::command]
async fn list_skills(
    skills: State<'_, ai::skills::SkillRegistry>,
) -> Result<Vec<ai::skills::SkillDefinition>, String> {
    Ok(skills.list_all().await)
}

#[tauri::command]
async fn list_plugins(
    plugins: State<'_, ai::plugins::PluginManager>,
) -> Result<Vec<ai::plugins::PluginInfo>, String> {
    Ok(plugins.list().await)
}

#[tauri::command]
async fn toggle_plugin(
    name: String,
    enabled: bool,
    plugins: State<'_, ai::plugins::PluginManager>,
) -> Result<bool, String> {
    plugins.toggle(&name, enabled).await;
    Ok(enabled)
}

#[tauri::command]
async fn toggle_hook(
    name: String,
    enabled: bool,
    hooks: State<'_, ai::hooks::HookManager>,
) -> Result<bool, String> {
    hooks.toggle_hook(&name, enabled).await;
    Ok(enabled)
}

#[tauri::command]
async fn resolve_question(
    request_id: String,
    answers: String,
    store: State<'_, ai::tools::question::QuestionStore>,
) -> Result<bool, String> {
    Ok(store.resolve(&request_id, &answers).await)
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
