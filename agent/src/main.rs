mod file_watcher;
mod pty_manager;
mod tabbit;
mod ws_client;

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "modeler-agent")]
struct Args {
    #[arg(long, default_value = "ws://localhost:3001/agent")]
    server: String,

    #[arg(long)]
    token: String,

    #[arg(long)]
    project_id: String,

    #[arg(long, default_value_t = 38921)]
    tabbit_port: u16,

    #[arg(long, default_value = ".")]
    work_dir: PathBuf,

    #[arg(long, default_value = "claude")]
    claude_path: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    tracing::info!("Modeler Agent starting");
    tracing::info!("  server: {}", args.server);
    tracing::info!("  project: {}", args.project_id);

    ws_client::run(
        &args.server,
        &args.token,
        &args.project_id,
        args.tabbit_port,
        &args.work_dir,
        &args.claude_path,
    )
    .await
}
