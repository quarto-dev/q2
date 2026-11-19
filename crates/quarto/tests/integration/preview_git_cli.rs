//! Binary-level tests for the GE-4 startup warning (G28): `q2 preview` warns when the project directory is
//! bound by a VS Code extension folder (`.quarto-hub/project.json`), and still starts.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");
const WARNING: &str = "bound to a Quarto Hub folder";

/// Starts `q2 preview --no-browser` on `dir` with `RUST_LOG=warn`, waits until the server accepts connections,
/// stops it, and returns what it printed on stderr.
fn preview_stderr(dir: &Path) -> String {
    let log = dir.join("preview-stderr.log");
    let mut child = Command::new(Q2_BIN)
        .args(["preview", "--no-browser"])
        .arg(dir)
        .env("RUST_LOG", "warn")
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(&log).expect("stderr log"))
        .spawn()
        .expect("spawn q2 preview");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout piped"));
    let mut line = String::new();
    let boot_line = loop {
        line.clear();
        let n = stdout.read_line(&mut line).expect("read boot line");
        assert!(
            n > 0,
            "preview exited before printing its boot URL (the server must still start)"
        );
        if line.contains("→ http") {
            break line.trim().to_string();
        }
    };
    // `→ http://127.0.0.1:<port>/?page=index.qmd`
    let port: u16 = boot_line
        .split("127.0.0.1:")
        .nth(1)
        .expect("boot URL has a loopback port")
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("boot URL port parses");
    let deadline = Instant::now() + Duration::from_secs(10);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            Instant::now() < deadline,
            "preview port {port} never accepted a connection"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    child.kill().expect("stop preview");
    child.wait().expect("reap preview");
    std::fs::read_to_string(&log).expect("read stderr log")
}

fn project_with_marker(body: &str) -> tempfile::TempDir {
    let temp = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(temp.path().join("index.qmd"), "---\ntitle: T\n---\n\nHi.\n").unwrap();
    let meta = temp.path().join(".quarto-hub");
    std::fs::create_dir_all(&meta).unwrap();
    std::fs::write(meta.join("project.json"), body).unwrap();
    temp
}

#[test]
fn preview_warns_when_an_extension_bound_the_directory_and_still_starts() {
    let temp = project_with_marker("{}");
    let stderr = preview_stderr(temp.path());
    assert!(
        stderr.contains(WARNING),
        "expected the G28 warning; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains(".quarto-hub"),
        "the warning names the marker; stderr was:\n{stderr}"
    );
}

#[test]
fn preview_is_silent_for_an_editor_only_marker() {
    let temp = project_with_marker(r#"{"mode": "editor-only"}"#);
    let stderr = preview_stderr(temp.path());
    assert!(
        !stderr.contains(WARNING),
        "editor-only must not warn; stderr was:\n{stderr}"
    );
}
