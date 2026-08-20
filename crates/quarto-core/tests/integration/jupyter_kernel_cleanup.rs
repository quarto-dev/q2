//! bd-hxhnnlzs: Jupyter kernels must not outlive the work that spawned
//! them. Every kernel spawned during an engine execution (here driven
//! through `record_capture`, the same producer path `q2 render` /
//! `q2 preview` / `q2 provide-hub` use) must be shut down — process
//! killed, connection file removed — by the time the outermost kernel
//! scope ends, instead of surviving in the process-global daemon until
//! the process exits and the kernel reparents to PID 1.
//!
//! The test isolates kernel connection files via `JUPYTER_RUNTIME_DIR`
//! (honored by `runtimelib::dirs::runtime_dir()`), watches that
//! directory while the render runs to learn each kernel's ZeroMQ
//! ports, and then asserts that after `record_capture` returns nothing
//! is listening on those ports and no connection files remain.
//!
//! Tests skip when the jupyter engine isn't installed — same gating as
//! engine_error_policy.rs.

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::record_capture;
use quarto_core::project::ProjectContext;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn engine_available(name: &str) -> bool {
    EngineRegistry::default()
        .get(name)
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
/// each distinct file's ports. Files are written atomically enough for
/// this purpose (a few hundred bytes); a partial read simply retries
/// on the next tick because the entry is only recorded once it parses.
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

/// True while something accepts TCP connections on `port` (the ZeroMQ
/// sockets of a live kernel do; a killed kernel's ports refuse).
fn port_is_open(port: u16) -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(250)).is_ok()
}

/// Wait (bounded) for a condition that becomes true once the kernel is
/// fully torn down; returns whether it did.
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
fn record_capture_shuts_down_spawned_kernels() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let runtime_dir = dir.path().join("jupyter-runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();

    // Isolate connection files so the watcher only sees kernels this
    // test spawned. Safe: nextest runs each test in its own process,
    // and the daemon reads the variable after this point.
    // lint note: `set_var` is unsafe in edition 2024 because of
    // concurrent readers; this test's process is single-threaded here.
    unsafe { std::env::set_var("JUPYTER_RUNTIME_DIR", &runtime_dir) };

    let qmd_path = dir.path().join("doc.qmd");
    std::fs::write(
        &qmd_path,
        "---\ntitle: Kernel cleanup\nengine: jupyter\n---\n\n```{python}\nprint(\"cleanup-test\")\n```\n",
    )
    .unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&qmd_path, runtime.as_ref()).unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let watcher = watch_connection_files(runtime_dir.clone(), stop.clone());

    let captures = pollster::block_on(record_capture(&qmd_path, &project, runtime, None))
        .expect("render of a healthy python cell succeeds");
    assert!(!captures.is_empty(), "engine capture was produced");

    stop.store(true, Ordering::Relaxed);
    let observed = watcher.join().unwrap();

    // The engine must actually have spawned a kernel, otherwise the
    // assertions below are vacuous.
    assert!(
        !observed.is_empty(),
        "watcher saw no kernel-*.json — the jupyter engine did not run \
         (JUPYTER_RUNTIME_DIR not honored, or the render skipped execution)"
    );

    // bd-hxhnnlzs: by the time record_capture returns, every spawned
    // kernel must be dead (ports closed) and its connection file gone.
    for kernel in &observed {
        for (label, port) in [
            ("shell", kernel.shell_port),
            ("control", kernel.control_port),
        ] {
            assert!(
                eventually(Duration::from_secs(5), || !port_is_open(port)),
                "kernel {} still listening on {} port {} after record_capture \
                 returned — the kernel process leaked (bd-hxhnnlzs)",
                kernel.file_name,
                label,
                port,
            );
        }
    }
    assert!(
        eventually(Duration::from_secs(5), || {
            remaining_connection_files(&runtime_dir).is_empty()
        }),
        "stale connection files left in JUPYTER_RUNTIME_DIR after \
         record_capture returned: {:?} (bd-hxhnnlzs)",
        remaining_connection_files(&runtime_dir),
    );
}
