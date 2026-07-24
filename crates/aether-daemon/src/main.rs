mod protocol;
mod server;
mod task_runner;

use aether_core::ModelRouter;
use aether_db::Database;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

pub struct DaemonState {
    pub db: Database,
    pub router: ModelRouter,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("aether_daemon=info".parse()?))
        .init();

    let db_path = std::env::var("AETHER_DB_PATH").unwrap_or_else(|_| {
        let home = dirs_home();
        format!("{}/.aether/aether.db", home)
    });

    if let Some(parent) = PathBuf::from(&db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let db = Database::open(&db_path)?;
    let router = ModelRouter::from_env()?;

    let state = Arc::new(DaemonState { db, router });
    let addr = aether_core::default_daemon_addr();

    tracing::info!("aether-daemon listening on {}", addr);
    tracing::info!("database: {}", db_path);
    if std::env::var("AETHER_BYOK_PROVIDER").is_ok() {
        tracing::info!("router: BYOK via Keychain ({})", std::env::var("AETHER_BYOK_PROVIDER").unwrap_or_default());
    } else {
        let model = std::env::var("AETHER_CHAT_MODEL").unwrap_or_else(|_| "qwen2.5:3b".to_string());
        tracing::info!("router: Ollama ({})", model);
    }

    server::serve(addr, state).await
}

fn dirs_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
}
