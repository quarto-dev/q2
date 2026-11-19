//! Integration tests for `GET /api/preview/git-status` and the `--git` wiring (GE-4, G33).
//!
//! Each test boots a real preview server on a fixture project (a real temporary git repository for the `Track`
//! cases) and reads the route over HTTP. State changes are awaited by polling the route, never by fixed sleeps.

use std::net::TcpListener as StdTcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use quarto_hub::HubContext;
use quarto_preview::{PreviewConfig, PreviewGit, run_with_on_ready};
use tokio::sync::oneshot;

fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct Rig {
    port: u16,
    ctx: Arc<HubContext>,
    dir: PathBuf,
    _handle: tokio::task::JoinHandle<anyhow::Result<()>>,
    _project: tempfile::TempDir,
    _data: tempfile::TempDir,
}

impl Rig {
    async fn boot(mode: PreviewGit, as_repo: bool) -> Rig {
        let project = tempfile::TempDir::with_prefix("q2-preview-git-status-").unwrap();
        let dir = project.path().canonicalize().unwrap();
        std::fs::write(
            dir.join("index.qmd"),
            "---\ntitle: Git status\n---\n\nHello.\n",
        )
        .unwrap();
        if as_repo {
            git(&dir, &["init", "-q", "-b", "main"]);
            git(&dir, &["config", "user.email", "t@example.com"]);
            git(&dir, &["config", "user.name", "T"]);
            git(&dir, &["config", "core.autocrlf", "false"]);
            git(&dir, &["add", "-A"]);
            git(&dir, &["commit", "-q", "-m", "init"]);
        }
        let data = tempfile::TempDir::with_prefix("q2-preview-git-status-data-").unwrap();
        let port = pick_free_port();
        let config = PreviewConfig {
            git: mode,
            render: Default::default(),
            upstream: None,
            host: "127.0.0.1".to_string(),
            port,
            project_root: Some(dir.clone()),
            single_file: None,
            data_dir: data.path().to_path_buf(),
            spa_dir_override: None,
            engine_registry: None,
            engine_policy: Default::default(),
            record_sources: quarto_core::record::RecordSources::preview_default(),
            resource_html_files: Vec::new(),
            cache_dir: None,
            allow_edit: true,
            share: false,
            ui: Default::default(),
        };
        let (ready_tx, ready_rx) = oneshot::channel::<Arc<HubContext>>();
        let mut ready_tx = Some(ready_tx);
        let handle = tokio::spawn(async move {
            run_with_on_ready(config, move |ctx| {
                if let Some(tx) = ready_tx.take() {
                    let _ = tx.send(ctx);
                }
            })
            .await
        });
        let ctx = tokio::time::timeout(Duration::from_secs(10), ready_rx)
            .await
            .expect("server reached on_ready within 10s")
            .expect("on_ready callback fired");
        Rig {
            port,
            ctx,
            dir,
            _handle: handle,
            _project: project,
            _data: data,
        }
    }

    async fn status(&self) -> serde_json::Value {
        let url = format!("http://127.0.0.1:{}/api/preview/git-status", self.port);
        let resp = reqwest::get(&url).await.expect("GET succeeds");
        assert_eq!(resp.status(), 200, "the route exists");
        resp.json().await.expect("response is JSON")
    }

    /// Polls the route until `pred` holds; panics with the last body after 20 s.
    async fn until(
        &self,
        what: &str,
        pred: impl Fn(&serde_json::Value) -> bool,
    ) -> serde_json::Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let body = self.status().await;
            if pred(&body) {
                return body;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; last body: {body}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn lock_path(&self) -> PathBuf {
        let p = PathBuf::from(git(&self.dir, &["rev-parse", "--git-path", "index.lock"]));
        if p.is_absolute() { p } else { self.dir.join(p) }
    }
}

fn flag(body: &serde_json::Value, key: &str) -> bool {
    body[key]
        .as_bool()
        .unwrap_or_else(|| panic!("`{key}` is a bool in {body}"))
}

#[tokio::test]
async fn ignore_reports_nothing_tracked() {
    let rig = Rig::boot(PreviewGit::Ignore, false).await;
    let body = rig.status().await;
    assert!(!flag(&body, "tracking") && !flag(&body, "paused") && !flag(&body, "pending"));
    assert_eq!(body["markers"], serde_json::json!([]));
    assert_eq!(body["decisions"], serde_json::json!([]));
}

#[tokio::test]
async fn track_reports_tracking_and_the_index_lock_pauses_it() {
    let rig = Rig::boot(PreviewGit::Track, true).await;
    let body = rig.status().await;
    assert!(flag(&body, "tracking"), "{body}");
    assert!(!flag(&body, "paused"), "{body}");

    let lock = rig.lock_path();
    std::fs::write(&lock, "").unwrap();
    let body = rig.until("paused", |b| flag(b, "paused")).await;
    assert_eq!(body["markers"], serde_json::json!(["index.lock"]), "{body}");

    std::fs::remove_file(&lock).unwrap();
    rig.until("resumed", |b| !flag(b, "paused")).await;
}

#[tokio::test]
async fn a_held_decision_is_pending_and_a_switch_is_listed_afterwards() {
    let rig = Rig::boot(PreviewGit::Track, true).await;
    git(&rig.dir, &["switch", "-q", "-c", "other"]);
    std::fs::write(
        rig.dir.join("index.qmd"),
        "---\ntitle: Git status\n---\n\nOther.\n",
    )
    .unwrap();
    git(&rig.dir, &["commit", "-qam", "other"]);
    git(&rig.dir, &["switch", "-q", "main"]);
    rig.until("settled after setup", |b| !flag(b, "pending"))
        .await;

    let hooks = &rig.ctx.git_epochs().expect("tracking").hooks;
    hooks.hold_decisions.store(true, Ordering::SeqCst);
    git(&rig.dir, &["switch", "-q", "other"]);
    rig.until("pending while held", |b| flag(b, "pending"))
        .await;
    hooks.hold_decisions.store(false, Ordering::SeqCst);
    let body = rig
        .until("a flipped decision", |b| {
            !flag(b, "pending")
                && b["decisions"]
                    .as_array()
                    .is_some_and(|d| d.iter().any(|r| r["kind"] == "flipped"))
        })
        .await;
    let decisions = body["decisions"].as_array().unwrap();
    assert!(decisions.len() <= 20);
    let d = decisions.iter().find(|r| r["kind"] == "flipped").unwrap();
    assert!(
        d["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "index.qmd"),
        "{d}"
    );
    assert!(d["step"].is_string(), "the G9 step is reported: {d}");
    assert!(d.get("discarded").is_some(), "{d}");
}

#[tokio::test]
async fn a_commit_is_listed_as_moved_forward() {
    let rig = Rig::boot(PreviewGit::Track, true).await;
    std::fs::write(
        rig.dir.join("index.qmd"),
        "---\ntitle: Git status\n---\n\nEdited.\n",
    )
    .unwrap();
    rig.ctx.sync_all().await;
    git(&rig.dir, &["commit", "-qam", "edit"]);
    rig.until("a moved-forward decision", |b| {
        b["decisions"]
            .as_array()
            .is_some_and(|d| d.iter().any(|r| r["kind"] == "moved-forward"))
    })
    .await;
}
