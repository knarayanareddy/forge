//! Example TCP client for aether-daemon streaming.
use aether_core::default_daemon_addr;
use std::io::{BufRead, BufWrite, Write};
use std::net::TcpStream;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = default_daemon_addr();
    let mut stream = TcpStream::connect(&addr)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(60)))?;

    let request = r#"{"method":"run_task","params":{"prompt":"Reply with one word: forge"}}"#;
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let reader = std::io::BufReader::new(&stream);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        println!("{}", line);
        if line.contains("\"type\":\"done\"") || line.contains("\"type\":\"error\"") {
            break;
        }
    }

    Ok(())
}
