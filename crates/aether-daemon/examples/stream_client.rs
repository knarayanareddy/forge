use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::var("AETHER_DAEMON_ADDR")
        .unwrap_or_else(|_| aether_core::default_daemon_addr());

    let mut stream = TcpStream::connect_timeout(
        &addr.parse()?,
        Duration::from_secs(5),
    )?;
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;

    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Reply with one word: forge".to_string());

    let request = format!(
        r#"{{"method":"run_task","params":{{"prompt":"{}"}}}}"#,
        prompt.replace('\\', "\\\\").replace('"', "\\\"")
    );
    writeln!(stream, "{}", request)?;
    stream.flush()?;

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        println!("{}", line);
        if line.contains(r#""type":"done""#) || line.contains(r#""type":"error""#) {
            break;
        }
    }

    Ok(())
}
