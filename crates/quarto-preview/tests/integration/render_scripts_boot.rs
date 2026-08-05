//! Pre-render scripts run once at preview boot (bd-w348iu63, plan D7).
//!
//! `q2 preview` runs the project's `project.pre-render` scripts
//! exactly once, at server boot, alongside the eager-capture driver
//! in the on-ready hook. They are deliberately NOT re-run on file
//! changes (a documented deviation from Quarto 1's every-re-render
//! behavior), and post-render scripts never run in preview (there is
//! no materialized output dir in the preview loop).
//!
//! The fixture script appends a line to `boot.log` on every run, so
//! the test can assert both "ran at boot" and "did not run again
//! after a file change".

use std::net::TcpListener as StdTcpListener;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use quarto_preview::PreviewConfig;

fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

async fn wait_for_health(port: u16) {
    let url = format!("http://127.0.0.1:{port}/health");
    let deadline = Instant::now() + Duration::from_secs(10);
    let client = reqwest::Client::new();
    loop {
        if let Ok(resp) = client.get(&url).send().await
            && resp.status().is_success()
        {
            return;
        }
        if Instant::now() >= deadline {
            panic!("server didn't come up on port {port} within 10s");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Find a Python interpreter for the fixture script; `None` ⇒ skip.
fn find_python() -> Option<&'static str> {
    let candidates: &[&str] = if cfg!(windows) {
        &["python", "python3"]
    } else {
        &["python3", "python"]
    };
    for candidate in candidates {
        if let Ok(status) = Command::new(candidate)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            && status.success()
        {
            return Some(candidate);
        }
    }
    None
}

/// Poll until `path` exists (the boot script runs on a blocking
/// worker after the health endpoint is already up).
async fn wait_for_file(path: &Path, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        if Instant::now() >= deadline {
            panic!("{what} not created within 10s: {}", path.display());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_render_scripts_run_once_at_boot() {
    if find_python().is_none() {
        eprintln!("SKIP: no python interpreter on PATH");
        return;
    }

    let project = tempfile::TempDir::with_prefix("q2-preview-scripts-").unwrap();
    let project_dir = project
        .path()
        .canonicalize()
        .unwrap_or_else(|_| project.path().to_path_buf());
    std::fs::write(
        project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  pre-render: boot.py\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.join("boot.py"),
        "import os\nwith open(\"boot.log\", \"a\") as f:\n    f.write(os.environ.get(\"QUARTO_PROJECT_RENDER_ALL\", \"absent\") + \"\\n\")\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHome body.\n",
    )
    .unwrap();
    let data = tempfile::TempDir::with_prefix("q2-preview-scripts-data-").unwrap();

    let port = pick_free_port();
    let config = PreviewConfig {
        host: "127.0.0.1".to_string(),
        port,
        project_root: Some(project_dir.clone()),
        single_file: None,
        data_dir: data.path().to_path_buf(),
        spa_dir_override: None,
        engine_registry: None,
        engine_policy: Default::default(),
        resource_html_files: Vec::new(),
        cache_dir: None,
        allow_edit: false,
        share: false,
    };

    let server = tokio::spawn(async move {
        let _ = quarto_preview::run(config).await;
    });

    wait_for_health(port).await;

    // 1. The pre-render script ran at boot, with the full-render env.
    let log_path = project_dir.join("boot.log");
    wait_for_file(&log_path, "boot.log").await;
    let log = std::fs::read_to_string(&log_path).unwrap();
    assert_eq!(
        log, "1\n",
        "pre-render script should run exactly once at boot with QUARTO_PROJECT_RENDER_ALL=1"
    );

    // 2. A file change does NOT re-run the scripts (decided deviation
    //    from Q1). Give the watcher + any (incorrect) re-run a real
    //    chance to fire before asserting.
    std::fs::write(
        project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHome body — edited.\n",
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let log = std::fs::read_to_string(&log_path).unwrap();
    assert_eq!(
        log, "1\n",
        "pre-render scripts must not re-run on file changes"
    );

    server.abort();
}
