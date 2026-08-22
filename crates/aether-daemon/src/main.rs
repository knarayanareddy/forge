use aether_core::{discover_registry_path, ModelRouter};
use aether_daemon::headless::run_headless_cli;
use aether_db::Database;
use aether_daemon::server;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("aether_daemon=info".parse()?))
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--json") {
        process::exit(run_headless_cli(args).await.as_i32());
    }

    let db_path = std::env::var("AETHER_DB_PATH").unwrap_or_else(|_| {
        let home = dirs_home();
        format!("{}/.aether/aether.db", home)
    });

    if let Some(parent) = PathBuf::from(&db_path).parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }

    let db = Database::open(&db_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600))?;
    }
    let retention_days = std::env::var("AETHER_RETENTION_DAYS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(30)
        .clamp(1, 3_650);
    db.purge_expired_data(retention_days)?;
    let router = ModelRouter::from_env()?;

    #[cfg(target_os = "macos")]
    let auth_token = aether_core::ensure_daemon_auth_token()?;
    #[cfg(not(target_os = "macos"))]
    let auth_token = std::env::var("AETHER_DAEMON_AUTH_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
        .ok_or("AETHER_DAEMON_AUTH_TOKEN is mandatory on non-macOS")?;

    if std::env::var_os("AETHER_LOG_INTEGRITY_KEY").is_none() {
        std::env::set_var("AETHER_LOG_INTEGRITY_KEY", &auth_token);
    }
    aether_daemon::session_log::SessionLogWriter::from_env()
        .purge_expired_logs(retention_days)?;

    let state = Arc::new(aether_daemon::DaemonState {
        db,
        router,
        auth_token,
    });
    let addr = aether_core::default_daemon_addr();
    let socket_addr: std::net::SocketAddr = addr
        .parse()
        .map_err(|_| format!("invalid AETHER_DAEMON_ADDR: {addr}"))?;
    if !socket_addr.ip().is_loopback() {
        return Err(format!(
            "refusing non-loopback daemon bind {addr}; remote IPC requires a separate TLS/mTLS gateway"
        )
        .into());
    }

    tracing::info!("aether-daemon listening on {}", addr);
    tracing::info!("database: {}", db_path);
    if !state.auth_token.is_empty() {
        tracing::info!("daemon auth token enabled (Keychain + IPC gate)");
    }
    if std::env::var("AETHER_BYOK_PROVIDER").is_ok() {
        tracing::info!(
            "router: BYOK via Keychain ({})",
            std::env::var("AETHER_BYOK_PROVIDER").unwrap_or_default()
        );
    } else if let Some(reg_path) = discover_registry_path() {
        tracing::info!("router: profile {} ({})", state.router.active_profile_label(), reg_path.display());
    } else {
        tracing::info!("router: {}", state.router.active_profile_label());
    }

    server::serve(addr, state).await
}

fn dirs_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
}
