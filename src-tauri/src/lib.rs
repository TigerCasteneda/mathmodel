mod agent;

use agent::state::AgentState;
use std::path::PathBuf;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let work_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            app.manage(AgentState {
                pty_tx: std::sync::Mutex::new(None),
                work_dir: std::sync::Mutex::new(work_dir),
                app_handle: app.handle().clone(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            agent::commands::pty_spawn,
            agent::commands::pty_write,
            agent::commands::pty_resize,
            agent::commands::pty_kill,
            agent::commands::list_files,
            agent::commands::read_file,
            agent::commands::create_file,
            agent::commands::change_work_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
