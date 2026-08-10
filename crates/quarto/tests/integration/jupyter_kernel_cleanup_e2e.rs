//! bd-hxhnnlzs: `q2 render` must not orphan Jupyter kernels. Before
//! this fix, every render of a jupyter document left its kernel
//! running forever (reparented to PID 1, ~6 listening sockets, stale
//! `kernel-*.json` in the Jupyter runtime dir) because the kernel
//! daemon lives in a process-global static whose sessions are never
//! shut down and never dropped.
//!
//! This drives the real `q2` binary the way a user would, isolating
//! kernel connection files via `JUPYTER_RUNTIME_DIR` (honored by
//! `runtimelib::dirs::runtime_dir()`). A watcher thread records each
//! kernel's ports while the render runs; after the process exits the
//! test asserts every observed kernel is dead and its connection file
//! removed.
//!
//! The project fixture has TWO python documents in the same directory
//! on purpose: they share a daemon session key, and the render-scoped
//! kernel scope must keep the kernel warm across documents (exactly
//! one kernel observed) while still shutting it down at the end.
//!
//! Skips when the jupyter engine isn't installed — same gating as the
//! quarto-core engine tests.

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn jupyter_available() -> bool {
    quarto_core::engine::EngineRegistry::default()
        .get("jupyter")
        .is_some_and(|e| e.is_available())
}

/// Ports recorded from one observed `kernel-*.json` connection file.
#[derive(Debug, Clone)]
struct ObservedKernel {
    file_name: String,
    shell_port: u16,
    control_port: u16,
}

/// Poll `dir` for `kernel-*.json` files until `stop` is set, recording
/// each distinct file's ports. A partially-written file simply fails
/// to parse and retries on the next tick.
fn watch_connection_files(
    dir: PathBuf,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<Vec<ObservedKernel>> {
    std::thread::spawn(move || {
        let mut seen: Vec<ObservedKernel> = Vec::new();
        while !stop.load(Ordering::Relaxed) {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if !(name.starts_with("kernel-") && name.ends_with(".json")) {
                        continue;
                    }
                    if seen.iter().any(|k| k.file_name == name) {
                        continue;
                    }
                    let Ok(text) = std::fs::read_to_string(entry.path()) else {
                        continue;
                    };
                    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };
                    let port = |key: &str| {
                        json.get(key)
                            .and_then(|v| v.as_u64())
                            .map_or(0, |p| p as u16)
                    };
                    let (shell_port, control_port) = (port("shell_port"), port("control_port"));
                    if shell_port != 0 && control_port != 0 {
                        seen.push(ObservedKernel {
                            file_name: name,
                            shell_port,
                            control_port,
                        });
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        seen
    })
}

/// True while something accepts TCP connections on `port`.
fn port_is_open(port: u16) -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(250)).is_ok()
}

/// Wait (bounded) for a teardown condition; returns whether it held.
fn eventually(deadline: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    loop {
        if cond() {
            return true;
        }
        if start.elapsed() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn remaining_connection_files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("kernel-") && n.ends_with(".json"))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn render_leaves_no_kernel_behind() {
    if !jupyter_available() {
        eprintln!("Skipping test: jupyter not available");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let project_dir = dir.path().join("proj");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("_quarto.yml"),
        "project:\n  type: website\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.join("a.qmd"),
        "---\ntitle: a\nengine: jupyter\n---\n\n```{python}\nprint(\"doc-a\")\n```\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.join("b.qmd"),
        "---\ntitle: b\nengine: jupyter\n---\n\n```{python}\nprint(\"doc-b\")\n```\n",
    )
    .unwrap();

    let runtime_dir = dir.path().join("jupyter-runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let watcher = watch_connection_files(runtime_dir.clone(), stop.clone());

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_q2"))
        .arg("render")
        .arg(&project_dir)
        .env("JUPYTER_RUNTIME_DIR", &runtime_dir)
        .output()
        .expect("q2 render runs");

    stop.store(true, Ordering::Relaxed);
    let observed = watcher.join().unwrap();

    assert!(
        output.status.success(),
        "q2 render failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    // Both documents actually executed their cells.
    for (doc, needle) in [("a.html", "doc-a"), ("b.html", "doc-b")] {
        let html =
            std::fs::read_to_string(project_dir.join("_site").join(doc)).expect("rendered output");
        assert!(html.contains(needle), "{doc} contains executed output");
    }

    // The render-scoped kernel pool keeps one kernel warm across the
    // two documents (same kernel + working dir → same session); more
    // than one observed kernel means per-document restarts crept in.
    assert_eq!(
        observed.len(),
        1,
        "expected the two python docs to share one kernel, observed: {observed:?}"
    );

    // bd-hxhnnlzs: after the q2 process exits, the kernel must be dead
    // and its connection file removed.
    let kernel = &observed[0];
    for (label, port) in [
        ("shell", kernel.shell_port),
        ("control", kernel.control_port),
    ] {
        assert!(
            eventually(Duration::from_secs(5), || !port_is_open(port)),
            "kernel {} still listening on {} port {} after q2 render exited — \
             the kernel process leaked (bd-hxhnnlzs)",
            kernel.file_name,
            label,
            port,
        );
    }
    assert!(
        eventually(Duration::from_secs(5), || {
            remaining_connection_files(&runtime_dir).is_empty()
        }),
        "stale connection files left after q2 render exited: {:?} (bd-hxhnnlzs)",
        remaining_connection_files(&runtime_dir),
    );
}
