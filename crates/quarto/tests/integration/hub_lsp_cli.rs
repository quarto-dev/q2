//! `q2 hub-lsp` end to end: the real binary joins a real `q2 hub` and speaks
//! LSP on stdio.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Kills the child when dropped, so a failing test leaves no process behind.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A standalone `q2 hub` on a free loopback port, once it accepts
/// connections.
fn spawn_hub(data_dir: &std::path::Path) -> (Reaped, u16) {
    let port = free_port();
    let hub = Command::new(env!("CARGO_BIN_EXE_q2"))
        .args(["hub", "--no-project", "--host", "127.0.0.1", "--port"])
        .arg(port.to_string())
        .arg("--data-dir")
        .arg(data_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn q2 hub");
    let hub = Reaped(hub);
    let deadline = Instant::now() + Duration::from_secs(30);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "q2 hub never started listening");
        std::thread::sleep(Duration::from_millis(50));
    }
    (hub, port)
}

fn send(stdin: &mut impl Write, message: Value) {
    let body = message.to_string();
    write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    stdin.flush().unwrap();
}

/// The next LSP message on `stdout`.
fn receive(stdout: &mut BufReader<ChildStdout>) -> Value {
    let mut length = None;
    loop {
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length: ") {
            length = Some(value.parse::<usize>().unwrap());
        }
    }
    let mut body = vec![0; length.expect("a Content-Length header")];
    stdout.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// The response to request `id`, skipping notifications.
fn response(stdout: &mut BufReader<ChildStdout>, id: i64) -> Value {
    loop {
        let message = receive(stdout);
        if message["id"] == json!(id) {
            return message;
        }
    }
}

#[test]
fn hub_lsp_initializes_against_a_hub_and_exits_on_exit_with_stdin_still_open() {
    let dir = tempfile::tempdir().unwrap();
    let (_hub, port) = spawn_hub(&dir.path().join("hub-data"));
    let secret = dir.path().join("relay-secret");
    std::fs::write(&secret, "not-a-secret\n").unwrap();

    let mut lsp = Command::new(env!("CARGO_BIN_EXE_q2"))
        .arg("hub-lsp")
        .arg("--server")
        .arg(format!("ws://127.0.0.1:{port}/ws"))
        .arg("--header-file")
        .arg(format!("X-Relay-Key={}", secret.display()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn q2 hub-lsp");
    let mut stdin = lsp.stdin.take().unwrap();
    let mut stdout = BufReader::new(lsp.stdout.take().unwrap());
    let mut lsp = Reaped(lsp);

    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
               "params": {"capabilities": {}, "workspaceFolders": []}}),
    );
    let initialized = response(&mut stdout, 1);
    assert_eq!(
        initialized["result"]["serverInfo"]["name"],
        json!("quarto-hub-lsp")
    );
    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
    );

    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "id": 2, "method": "shutdown"}),
    );
    assert_eq!(response(&mut stdout, 2)["result"], Value::Null);
    send(&mut stdin, json!({"jsonrpc": "2.0", "method": "exit"}));

    // `stdin` stays open: a client may send `exit` and keep its end of the
    // pipe. The server must exit on the notification alone.
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = lsp.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "q2 hub-lsp did not exit after the exit notification"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "q2 hub-lsp exited with {status}");
    drop(stdin);
}
