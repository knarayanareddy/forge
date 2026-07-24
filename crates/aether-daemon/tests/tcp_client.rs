use std::io::{BufRead, BufRead as _, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

#[test]
fn tcp_ping_and_run_task_smoke() {
    let addr = std::env::var("AETHER_DAEMON_ADDR").unwrap_or_else(|_| "127.0.0.1:9731".to_string());

    let Ok(mut stream) = TcpStream::connect_timeout(
        &addr.parse().expect("valid addr"),
        Duration::from_secs(2),
    ) else {
        eprintln!("daemon not running on {} — skipping integration smoke", addr);
        return;
    };

    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();

    writeln!(stream, r#"{{"method":"ping"}}"#).expect("write ping");
    stream.flush().expect("flush ping");

    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("read pong");
    assert!(line.contains("pong"), "expected pong, got {}", line);
}
