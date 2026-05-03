//! `PublishHost` — the side-effect surface providers receive.
//!
//! All UI, network, and process-spawn affordances that providers
//! need go through this trait. Keeping them here means:
//!
//! - Tests can inject a recording host instead of touching the
//!   network or opening browsers.
//! - A future WASM-bridged provider receives a browser-flavored
//!   host (`window.open`, `fetch`, modal prompts) without needing
//!   any other shape change.
//! - Process exec is *deliberately not* on this trait: native-only
//!   providers (gh-pages → `git`) call `Command` directly inside
//!   their impls. WASM providers naturally lack the affordance.

use async_trait::async_trait;
use std::io::Read;
use std::sync::{Arc, Mutex};

use crate::types::{PublishEvent, PublishOutcome};

/// Side-effect surface for a provider.
#[async_trait]
pub trait PublishHost: Send + Sync {
    /// Emit a progress event. The native host renders these as
    /// human-readable lines (or NDJSON under `--json`); a future
    /// WASM host can pipe them into the UI.
    async fn emit(&self, event: PublishEvent);

    /// Open a URL in the user's browser. May fail or be a no-op
    /// (e.g. headless environments). Errors are non-fatal — the
    /// caller logs and proceeds.
    async fn open_url(&self, url: &str) -> Result<(), anyhow::Error>;

    /// Perform a GET against `url`, returning (status, body). Used
    /// by `verify` for the `.nojekyll` poll. Implementations may
    /// short-circuit network IO in tests.
    async fn http_get(&self, url: &str) -> Result<HttpResponse, anyhow::Error>;
}

/// Minimal HTTP response shape — providers don't need headers, just
/// status + body.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Native (CLI) implementation of `PublishHost`.
///
/// Browser-open uses the platform default opener (`open`,
/// `xdg-open`, `cmd /c start`). HTTP via `ureq` (sync, leaner than
/// `reqwest`). Events go to stderr — human-readable by default,
/// NDJSON when `json_output` is set.
///
/// **Phase 0:** the HTTP and browser methods are minimal stubs that
/// return errors so the unimplemented providers don't accidentally
/// "succeed" by making a real network call. Phase 1 fleshes them
/// out alongside the gh-pages `verify` step.
pub struct NativeHost {
    json_output: bool,
    /// Captured events for testing. None in production.
    captured: Option<Arc<Mutex<Vec<PublishEvent>>>>,
}

impl NativeHost {
    pub fn new(json_output: bool) -> Self {
        Self {
            json_output,
            captured: None,
        }
    }

    /// Build a host that records every emitted event into a shared
    /// vector. Returns the host plus the shared handle so a test
    /// can inspect what was emitted.
    pub fn recording() -> (Self, Arc<Mutex<Vec<PublishEvent>>>) {
        let captured = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                json_output: false,
                captured: Some(captured.clone()),
            },
            captured,
        )
    }

    pub fn json_output(&self) -> bool {
        self.json_output
    }

    /// Render an event to stderr. Pulled out so tests can pin the
    /// emitted shape without driving the trait.
    pub fn render_event(&self, event: &PublishEvent) -> String {
        if self.json_output {
            // NDJSON: one JSON object per line, no trailing newline
            // (the caller adds one).
            serde_json::to_string(event).unwrap_or_else(|e| {
                format!(
                    "{{\"kind\":\"event-serialization-error\",\"error\":{}}}",
                    serde_json::to_string(&e.to_string()).unwrap()
                )
            })
        } else {
            human_event(event)
        }
    }

    /// Render the final outcome on stdout. Called by the top-level
    /// driver, not by providers.
    pub fn render_outcome(&self, outcome: &PublishOutcome) -> String {
        if self.json_output {
            serde_json::to_string(outcome).unwrap_or_else(|e| {
                format!(
                    "{{\"error\":{{\"code\":\"Q-PUBLISH-OTHER\",\"message\":{}}}}}",
                    serde_json::to_string(&e.to_string()).unwrap()
                )
            })
        } else {
            human_outcome(outcome)
        }
    }
}

#[async_trait]
impl PublishHost for NativeHost {
    async fn emit(&self, event: PublishEvent) {
        if let Some(captured) = &self.captured {
            captured.lock().unwrap().push(event.clone());
        }
        eprintln!("{}", self.render_event(&event));
    }

    async fn open_url(&self, url: &str) -> Result<(), anyhow::Error> {
        platform_open_url(url)
    }

    async fn http_get(&self, url: &str) -> Result<HttpResponse, anyhow::Error> {
        // ureq is sync; we're inside an `async fn` driven by
        // pollster, so the synchronous call doesn't hurt — the only
        // alternative is to pull in tokio + reqwest which is much
        // heavier for the single-request `verify` step we have.
        let url = url.to_string();
        let response = std::thread::scope(|s| {
            s.spawn(|| -> Result<HttpResponse, anyhow::Error> {
                match ureq::get(&url).call() {
                    Ok(resp) => {
                        let status = resp.status();
                        let mut body = Vec::new();
                        resp.into_reader()
                            .take(32 * 1024) // .nojekyll-sized response cap
                            .read_to_end(&mut body)?;
                        Ok(HttpResponse { status, body })
                    }
                    Err(ureq::Error::Status(status, resp)) => {
                        // ureq reports non-2xx as `Status` errors.
                        // We expose them as a regular response so
                        // callers (the gh-pages probe) can match
                        // on 404 vs 5xx.
                        let mut body = Vec::new();
                        let _ = resp.into_reader().take(32 * 1024).read_to_end(&mut body);
                        Ok(HttpResponse { status, body })
                    }
                    Err(e) => Err(anyhow::anyhow!("http error: {e}")),
                }
            })
            .join()
            .map_err(|_| anyhow::anyhow!("http request panicked"))?
        })?;
        Ok(response)
    }
}

/// Open `url` in the platform's default browser.
fn platform_open_url(url: &str) -> Result<(), anyhow::Error> {
    use std::process::Command;
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "linux")]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = url;
        return Err(anyhow::anyhow!("no platform browser opener available"));
    }
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        let status = cmd.status()?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "browser opener exited non-zero (code {:?})",
                status.code()
            ))
        }
    }
}

fn human_event(event: &PublishEvent) -> String {
    use PublishEvent::*;
    match event {
        RenderStart => "Rendering for publish...".to_string(),
        RenderProgress { rendered, total } => {
            format!("Rendered {rendered} of {total}")
        }
        RenderComplete => "Render complete.".to_string(),
        PrepareStart { provider } => format!("Preparing {provider} publish..."),
        Plan { provider, actions } => {
            let mut s = format!("Plan for {provider}:\n");
            for a in actions {
                s.push_str("  - ");
                s.push_str(&human_action(a));
                s.push('\n');
            }
            // Trim trailing newline.
            s.trim_end().to_string()
        }
        CommitStart { provider } => format!("Committing {provider} publish..."),
        CommitComplete { provider } => format!("Committed {provider} publish."),
        DeployWaiting { url } => format!("Waiting for deploy to be live at {url}..."),
        DeployVerified { url } => format!("Deploy verified live at {url}."),
        Note { message } => message.clone(),
    }
}

fn human_action(action: &crate::types::PublishAction) -> String {
    use crate::types::PublishAction::*;
    match action {
        Render { project_dir } => format!("Render {}", project_dir.display()),
        CreateRemoteBranch { branch } => format!("Create remote branch '{branch}'"),
        PushBranch {
            remote,
            branch,
            commit,
        } => format!("Push commit {commit} to {remote}/{branch}"),
        UploadFiles { count, bytes } => {
            format!("Upload {count} files ({bytes} bytes)")
        }
        WaitForDeploy { url, timeout_secs } => {
            format!("Wait up to {timeout_secs}s for deploy at {url}")
        }
        Note { message } => message.clone(),
    }
}

fn human_outcome(outcome: &PublishOutcome) -> String {
    let mut s = String::new();
    if outcome.dry_run {
        s.push_str(&format!(
            "Dry-run for {}: would have published.\n",
            outcome.provider
        ));
    } else {
        s.push_str(&format!("Published via {}.\n", outcome.provider));
    }
    if let Some(url) = &outcome.url {
        s.push_str(&format!("URL: {url}\n"));
    }
    if let Some(commit) = &outcome.summary.commit {
        s.push_str(&format!("Commit: {commit}\n"));
    }
    s.push_str(&format!(
        "Files: {} ({} bytes)\n",
        outcome.summary.file_count, outcome.summary.bytes
    ));
    if outcome.verified {
        s.push_str("Deploy verified live.\n");
    }
    s.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    #[test]
    fn render_event_in_json_mode_emits_one_line_of_ndjson() {
        let host = NativeHost::new(true);
        let line = host.render_event(&PublishEvent::RenderProgress {
            rendered: 3,
            total: 10,
        });
        assert!(!line.contains('\n'), "got {line}");
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["kind"], "render-progress");
        assert_eq!(parsed["rendered"], 3);
        assert_eq!(parsed["total"], 10);
    }

    #[test]
    fn render_event_in_human_mode_emits_human_text() {
        let host = NativeHost::new(false);
        let line = host.render_event(&PublishEvent::RenderProgress {
            rendered: 3,
            total: 10,
        });
        assert_eq!(line, "Rendered 3 of 10");
    }

    #[test]
    fn render_outcome_in_json_mode_serializes_outcome() {
        let host = NativeHost::new(true);
        let outcome = PublishOutcome {
            provider: "gh-pages".into(),
            record: None,
            url: Some("https://example.com/".parse().unwrap()),
            admin_url: None,
            summary: PublishSummary {
                commit: Some("abc123".into()),
                deploy_id: None,
                file_count: 5,
                bytes: 1024,
            },
            verified: true,
            dry_run: false,
        };
        let line = host.render_outcome(&outcome);
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["provider"], "gh-pages");
        assert_eq!(parsed["url"], "https://example.com/");
        assert_eq!(parsed["summary"]["commit"], "abc123");
        assert_eq!(parsed["summary"]["file_count"], 5);
        assert_eq!(parsed["summary"]["bytes"], 1024);
        assert_eq!(parsed["verified"], true);
        assert_eq!(parsed["dry_run"], false);
    }

    #[test]
    fn render_outcome_in_human_mode_includes_url_and_commit() {
        let host = NativeHost::new(false);
        let outcome = PublishOutcome {
            provider: "gh-pages".into(),
            record: None,
            url: Some("https://example.com/".parse().unwrap()),
            admin_url: None,
            summary: PublishSummary {
                commit: Some("abc123".into()),
                deploy_id: None,
                file_count: 5,
                bytes: 1024,
            },
            verified: true,
            dry_run: false,
        };
        let text = host.render_outcome(&outcome);
        assert!(text.contains("Published via gh-pages"));
        assert!(text.contains("https://example.com/"));
        assert!(text.contains("abc123"));
        assert!(text.contains("5"));
        assert!(text.contains("1024"));
        assert!(text.contains("Deploy verified live"));
    }

    #[test]
    fn dry_run_outcome_says_dry_run() {
        let host = NativeHost::new(false);
        let outcome = PublishOutcome::dry_run(
            "gh-pages",
            PublishSummary {
                commit: Some("abc123".into()),
                deploy_id: None,
                file_count: 5,
                bytes: 1024,
            },
            None,
        );
        let text = host.render_outcome(&outcome);
        assert!(text.contains("Dry-run for gh-pages"));
    }

    #[test]
    fn recording_host_captures_emitted_events() {
        let (host, captured) = NativeHost::recording();
        // Run async via pollster.
        pollster::block_on(async {
            host.emit(PublishEvent::RenderStart).await;
            host.emit(PublishEvent::RenderComplete).await;
        });
        let events = captured.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], PublishEvent::RenderStart));
        assert!(matches!(events[1], PublishEvent::RenderComplete));
    }
}
