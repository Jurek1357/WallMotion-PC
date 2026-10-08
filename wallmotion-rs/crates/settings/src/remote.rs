//! Single-instance remote commands over TCP loopback.
//!
//! The Python app forwards CLI commands through a `QLocalServer`
//! (`wallmotion/instance.py`). The Rust port has no Qt, so it uses a
//! simpler channel that needs only std: the first instance binds
//! [`REMOTE_ADDR`] on 127.0.0.1 and later invocations connect, send one
//! JSON object line and exit. Payload keys match `cli::args_to_command`
//! (`set`, `stop`, `muted`, `volume`).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

/// Loopback address of the command server. High port, no IANA conflict.
pub const REMOTE_ADDR: &str = "127.0.0.1:37951";

/// Timeout for connect/read/write of a single command.
const IO_TIMEOUT: Duration = Duration::from_millis(2000);

/// Max bytes read from one connection (one JSON line fits easily).
const MAX_BYTES: usize = 64 * 1024;

/// Send a command line to the running instance. True when delivered.
pub fn send_command(line: &str) -> bool {
    send_to(REMOTE_ADDR, line)
}

fn send_to(addr: &str, line: &str) -> bool {
    let mut stream = match TcpStream::connect(addr) {
        Ok(s) => s,
        Err(_) => return false,
    };
    if stream.set_write_timeout(Some(IO_TIMEOUT)).is_err() {
        return false;
    }
    let mut payload = line.trim().to_string();
    payload.push('\n');
    stream.write_all(payload.as_bytes()).is_ok()
}

/// Start the command server in a background thread. `None` when the port
/// is already taken (another instance is listening).
pub fn start_server() -> Option<mpsc::Receiver<String>> {
    let listener = TcpListener::bind(REMOTE_ADDR).ok()?;
    Some(serve(listener))
}

fn serve(listener: TcpListener) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        if buf.len() > MAX_BYTES {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let text = String::from_utf8_lossy(&buf);
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if tx.send(line.to_string()).is_err() {
                    return; // app is gone
                }
            }
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_ephemeral_port() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let rx = serve(listener);
        assert!(send_to(&addr, r#"{"stop":true}"#));
        let got = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(got, r#"{"stop":true}"#);
    }

    #[test]
    fn send_to_closed_port_fails_quietly() {
        // Nothing listens here (TEST-NET-1, unroutable in practice too).
        assert!(!send_to("127.0.0.1:37950", "{}"));
    }
}
