//! CLI surface tests for `q2 preview`.
//!
//! Phase A of the q2-preview epic (bd-kw93). These tests pin the
//! *clap* surface of the new subcommand: arg names, defaults,
//! exit code. They do NOT exercise the server boot or SPA serving
//! — those are covered by `crates/quarto-preview/tests/smoke.rs`
//! (A.2) and `crates/quarto-preview/tests/boot.rs` (A.5).
//!
//! The point of pinning the CLI surface this early is that A.5 and
//! consumers of `q2 preview` will key off these flags; a rename or
//! removal is a breaking change worth catching in the unit tier
//! rather than at integration time.
//!
//! Cargo wires the binary path through `CARGO_BIN_EXE_q2`. No
//! `assert_cmd` dep needed.

use std::process::Command;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

#[test]
fn preview_help_exits_zero() {
    let output = Command::new(Q2_BIN)
        .args(["preview", "--help"])
        .output()
        .expect("spawn q2 preview --help");
    assert!(
        output.status.success(),
        "q2 preview --help should exit 0, got {:?}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn preview_help_advertises_phase_a_args() {
    let output = Command::new(Q2_BIN)
        .args(["preview", "--help"])
        .output()
        .expect("spawn q2 preview --help");
    let help = String::from_utf8(output.stdout).expect("help is UTF-8");

    // Each Phase A flag — see claude-notes/plans/2026-05-13-q2-preview-
    // phase-a.md §A.1 — plus later additions like `--browser`. If one
    // disappears or gets renamed, the user-facing contract changes;
    // fail noisily.
    for flag in [
        "--port",
        "--no-browser",
        "--browser",
        "--data-dir",
        "--preview-dir",
        "--no-project",
    ] {
        assert!(
            help.contains(flag),
            "`q2 preview --help` did not advertise {flag}; got:\n{help}"
        );
    }
}

#[test]
fn preview_help_accepts_positional_path() {
    // The positional `[path]` argument lets users name a project root
    // or single file explicitly. clap renders positionals in
    // `<...>`/`[...]` brackets in the Usage line; we don't pin the
    // exact rendering, just confirm the help reaches us cleanly so
    // a future regression in the optional-positional setup is loud.
    let output = Command::new(Q2_BIN)
        .args(["preview", "--help"])
        .output()
        .expect("spawn q2 preview --help");
    let help = String::from_utf8(output.stdout).expect("help is UTF-8");
    assert!(
        help.lines()
            .any(|l| l.contains("Usage:") && l.contains("preview")),
        "help output missing `Usage: ... preview ...` line:\n{help}"
    );
}

/// Ctrl-C on the host prints a shutdown line (live-share review
/// follow-up): the hub's own "initiating graceful shutdown" is
/// tracing::info and invisible at the CLI's default filter, so the
/// process used to just vanish — while `--join` guests have always had
/// the friendly "leaving the shared session" line. Boots the real
/// binary in --no-project mode, waits for the port to accept, sends
/// SIGINT, and checks the printed line and the clean exit. Unix-only:
/// relies on `kill -INT`.
#[cfg(unix)]
#[test]
fn preview_prints_shutdown_message_on_ctrl_c() {
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let temp = tempfile::TempDir::new().expect("tempdir");
    let mut child = Command::new(Q2_BIN)
        .args(["preview", "--no-project", "--no-browser"])
        .current_dir(temp.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn q2 preview");

    let mut stdout = BufReader::new(child.stdout.take().expect("stdout piped"));

    // Read to the boot URL line, then poll the port until the server
    // accepts — by then every Ctrl-C handler (the hub's own and the
    // CLI's printer) is armed, so the signal can't slip through a
    // startup gap.
    let mut line = String::new();
    let boot_line = loop {
        line.clear();
        let n = stdout.read_line(&mut line).expect("read boot line");
        assert!(n > 0, "preview exited before printing its boot URL");
        if line.contains("→ http") {
            break line.trim().to_string();
        }
    };
    let port: u16 = boot_line
        .rsplit(':')
        .next()
        .expect("boot URL has a port")
        .trim_end_matches('/')
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

    // The terminal Ctrl-C, delivered as SIGINT.
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("run kill -INT");
    assert!(status.success(), "kill -INT failed");

    // Draining stdout to EOF waits out the graceful shutdown (the
    // exiting process closes the pipe).
    let mut rest = String::new();
    stdout
        .read_to_string(&mut rest)
        .expect("drain stdout after SIGINT");
    let status = child.wait().expect("wait on child");
    assert!(
        status.success(),
        "preview must exit cleanly on Ctrl-C, got {status:?}"
    );
    assert!(
        rest.contains("Received Ctrl-C, shutting down the preview"),
        "missing the Ctrl-C shutdown line; got:\n{rest}"
    );
}

// ──────────────────────────────────────────────────────────────────
// `--static` mode (bd-sl79jjiq, plan
// `claude-notes/plans/2026-09-22-q2-preview-static.md` Phase 0).
// These pin the clap surface: the flags exist, the hub-mode flags
// are rejected alongside `--static`, and the static-only flags are
// rejected without it.
// ──────────────────────────────────────────────────────────────────

fn preview_output(args: &[&str], cwd: &std::path::Path) -> std::process::Output {
    Command::new(Q2_BIN)
        .arg("preview")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("spawn q2 preview")
}

#[test]
fn preview_help_advertises_static_flags() {
    let output = Command::new(Q2_BIN)
        .args(["preview", "--help"])
        .output()
        .expect("spawn q2 preview --help");
    let help = String::from_utf8(output.stdout).expect("help is UTF-8");
    for flag in ["--static", "--no-watch", "--no-navigate", "--to"] {
        assert!(
            help.contains(flag),
            "`q2 preview --help` did not advertise {flag}; got:\n{help}"
        );
    }
}

/// Every hub-mode flag is meaningless without the hub, so clap must
/// reject each one next to `--static` (exit 2 = usage error) rather
/// than letting the static server silently ignore it.
#[test]
fn preview_static_rejects_hub_mode_flags() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let dir = temp.path().to_str().expect("utf-8 tempdir");
    let cases: &[&[&str]] = &[
        &["--static", "--join", "q2previewticket"],
        &["--static", "--share"],
        &["--static", "--allow-edit"],
        &["--static", "--ui", "editor"],
        &["--static", "--no-project"],
        &["--static", "--data-dir", dir],
        &["--static", "--preview-dir", dir],
    ];
    for args in cases {
        let output = preview_output(args, temp.path());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?} should be a clap usage error (exit 2); stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("cannot be used with"),
            "{args:?} should report a flag conflict; stderr:\n{stderr}"
        );
    }
}

/// The static-only flags require `--static`; without it they are a
/// usage error, not a silent no-op.
#[test]
fn preview_static_only_flags_require_static() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let cases: &[&[&str]] = &[&["--no-watch"], &["--no-navigate"], &["--to", "html"]];
    for args in cases {
        let output = preview_output(args, temp.path());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?} without --static should be a clap usage error (exit 2); stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("--static"),
            "{args:?} should name the missing --static flag; stderr:\n{stderr}"
        );
    }
}

/// Only formats a static server can show are accepted. `typst`
/// renders (to a PDF via pandoc) but there is nothing to serve, so the
/// command refuses before starting anything — exit 1 with a message
/// naming the flag.
#[test]
fn preview_static_rejects_unservable_format() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(
        temp.path().join("doc.qmd"),
        "---\ntitle: T\n---\n\nHello.\n",
    )
    .expect("write fixture");
    let output = preview_output(&["--static", "--to", "typst", "doc.qmd"], temp.path());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "--static --to typst should exit 1; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("not supported with --static"),
        "stderr should say the format is not supported with --static; got:\n{stderr}"
    );
    assert!(
        !temp.path().join("doc.pdf").exists() && !temp.path().join("doc.typ").exists(),
        "no render must happen before the format check"
    );
}
