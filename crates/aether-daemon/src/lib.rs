pub mod automation;
pub mod automation_webhook;
pub mod checkpoint;
pub mod gateway;
pub mod session_fork;
pub mod headless;
pub mod ingest;
pub mod protocol;
pub mod server;
pub mod session_log;
pub mod task_runner;

use aether_core::ModelRouter;
use aether_db::Database;

pub struct DaemonState {
    pub db: Database,
    pub router: ModelRouter,
    pub auth_token: String,
}
