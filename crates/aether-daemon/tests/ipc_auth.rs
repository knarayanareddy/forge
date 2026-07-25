use aether_core::ModelRouter;
use aether_daemon::server::{ipc_auth_ok, serve};
use aether_daemon::DaemonState;
use aether_db::Database;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

fn read_one_event(stream: &TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("read line");
    line
}

fn spawn_test_server(state: Arc<DaemonState>) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(listener);

    let addr_str = addr.to_string();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        rt.block_on(async move {
            let _ = serve(addr_str, state).await;
        });
    });

    addr.to_string()
}

#[test]
fn ipc_register_automation_denied_without_auth() {
    let db = Database::open_in_memory().expect("db");
    let router = ModelRouter::from_env().expect("router");
    let token = "ipc-test-token-register".to_string();
    let state = Arc::new(DaemonState {
        db,
        router,
        auth_token: token,
    });

    let addr = spawn_test_server(Arc::clone(&state));
    std::thread::sleep(Duration::from_millis(200));

    let mut stream = TcpStream::connect_timeout(
        &addr.parse().expect("addr"),
        Duration::from_secs(2),
    )
    .expect("connect");
    stream.set_write_timeout(Some(Duration::from_secs(2))).ok();

    let req = r#"{"method":"register_automation","params":{"trigger_id":"t1","session_id":"s1","trigger_type":"cron"}}"#;
    writeln!(stream, "{}", req).expect("write");
    stream.flush().expect("flush");

    let line = read_one_event(&stream);
    assert!(
        line.contains("Invalid or missing auth_token"),
        "expected auth denial, got: {}",
        line
    );
}

#[test]
fn ipc_ping_allowed_without_auth_helper() {
    assert!(!ipc_auth_ok(None, "some-token"));
    assert!(ipc_auth_ok(None, ""));
}

#[test]
fn ipc_register_automation_succeeds_with_auth_no_auto_grant() {
    let db = Database::open_in_memory().expect("db");
    let router = ModelRouter::from_env().expect("router");
    let token = "ipc-test-token-ok".to_string();
    {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('s-auth', 'IPC test', 'active')",
            [],
        )
        .expect("session row");
    }
    let state = Arc::new(DaemonState {
        db,
        router,
        auth_token: token.clone(),
    });

    let addr = spawn_test_server(Arc::clone(&state));
    std::thread::sleep(Duration::from_millis(200));

    let mut stream = TcpStream::connect_timeout(
        &addr.parse().expect("addr"),
        Duration::from_secs(2),
    )
    .expect("connect");
    stream.set_write_timeout(Some(Duration::from_secs(2))).ok();

    let req = format!(
        r#"{{"method":"register_automation","params":{{"auth_token":"{}","trigger_id":"t-auth","session_id":"s-auth","trigger_type":"cron","task_prompt":"hello"}}}}"#,
        token
    );
    writeln!(stream, "{}", req).expect("write");
    stream.flush().expect("flush");

    let line = read_one_event(&stream);
    assert!(
        line.contains("automation_registered"),
        "expected registration, got: {}",
        line
    );
}

#[test]
fn ipc_register_automation_rejects_grant_automation_flag() {
    let db = Database::open_in_memory().expect("db");
    let router = ModelRouter::from_env().expect("router");
    let token = "ipc-test-token-grant".to_string();
    let state = Arc::new(DaemonState {
        db,
        router,
        auth_token: token.clone(),
    });

    let addr = spawn_test_server(Arc::clone(&state));
    std::thread::sleep(Duration::from_millis(200));

    let mut stream = TcpStream::connect_timeout(
        &addr.parse().expect("addr"),
        Duration::from_secs(2),
    )
    .expect("connect");
    stream.set_write_timeout(Some(Duration::from_secs(2))).ok();

    let req = format!(
        r#"{{"method":"register_automation","params":{{"auth_token":"{}","trigger_id":"t-grant","session_id":"s-grant","trigger_type":"cron","grant_automation":true}}}}"#,
        token
    );
    writeln!(stream, "{}", req).expect("write");
    stream.flush().expect("flush");

    let line = read_one_event(&stream);
    assert!(
        line.contains("grant_automation via IPC is forbidden"),
        "expected grant_automation rejection, got: {}",
        line
    );
}
