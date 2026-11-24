//! Previews hosted by the LSP (bd-foum4ql3; plan
//! `claude-notes/plans/2026-11-17-q2-vscode-extension.md`, D-C and D-E).
//!
//! `quarto/previewStart` asks for a preview of a document. Its answer is the
//! URL of a running `q2 preview --static` session. The server keeps **one
//! session per root** (the project root, or the document outside a
//! project), so a second document in the same project reuses the session
//! that is already serving it, and two sessions never render into one
//! output directory.
//!
//! This crate does not know how to run a preview: that lives in the `q2`
//! binary, which this crate cannot depend on. The binary hands the server a
//! [`PreviewLauncher`] through [`HostServices`]. The wire protocol does not
//! say where a session runs (in this process today; a child process or a
//! `q2` service manager later), so hosting can change without touching
//! editors.
//!
//! [`PreviewRegistry`] holds the sessions. It is transport-free: each call
//! returns the [`Message`]s to send, so it is tested without a connection.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lsp_server::{ErrorCode, Message, Notification, RequestId, Response};
use serde_json::{Value, json};

/// `quarto/previewStart` (request).
pub const PREVIEW_START: &str = "quarto/previewStart";
/// `quarto/previewStop` (request).
pub const PREVIEW_STOP: &str = "quarto/previewStop";
/// `quarto/previewStatus` (server → client notification).
pub const PREVIEW_STATUS: &str = "quarto/previewStatus";
/// The protocol version advertised as `experimental.quarto.preview`.
pub const PREVIEW_PROTOCOL_VERSION: u32 = 1;

/// What a host provides beyond analysis. `None` services are not
/// advertised.
#[derive(Clone, Default)]
pub struct HostServices {
    pub previews: Option<Arc<dyn PreviewLauncher>>,
}

/// One preview to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRequest {
    /// The document (or directory) to preview.
    pub path: PathBuf,
    /// Output format, as `q2 preview --to`. `None`: the document's own.
    pub format: Option<String>,
}

/// What a session reports. Delivered from the session's own threads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewEvent {
    /// The boot render finished and the server is listening on `url`
    /// (the page of the document that started the session).
    Ready {
        url: String,
        root: PathBuf,
    },
    RenderStart,
    RenderStop {
        ok: bool,
        errors: usize,
        warnings: usize,
        text: String,
    },
    /// The session ended on its own (boot failure, server error).
    Ended {
        error: Option<String>,
    },
}

/// Delivers a session's events to the server loop.
pub type PreviewEventSink = Box<dyn Fn(PreviewEvent) + Send + Sync>;

/// Starts preview sessions. Implemented by the `q2` binary.
pub trait PreviewLauncher: Send + Sync {
    /// The root a preview of `req` would serve. Requests with the same key
    /// share one session.
    fn session_key(&self, req: &PreviewRequest) -> PathBuf;

    /// Start a session. It reports through `events`, and must send
    /// [`PreviewEvent::Ready`] or [`PreviewEvent::Ended`] eventually.
    fn start(
        &self,
        req: PreviewRequest,
        events: PreviewEventSink,
    ) -> Result<Box<dyn PreviewSession>, String>;
}

/// A running session.
pub trait PreviewSession: Send {
    /// The URL of `path`'s page, if the last render produced one.
    fn url_for(&self, path: &Path) -> Option<String>;
    /// Stop the session. Returns once it is asked to stop; the server
    /// closes in the background.
    fn stop(self: Box<Self>);
}

enum State {
    /// Waiting for the boot render; each waiter is a `previewStart`
    /// request and the document it asked for.
    Booting(Vec<(RequestId, PathBuf)>),
    Ready {
        url: String,
    },
}

struct Entry {
    key: PathBuf,
    format: Option<String>,
    session: Box<dyn PreviewSession>,
    state: State,
}

/// The server's preview sessions, by id.
pub struct PreviewRegistry {
    launcher: Arc<dyn PreviewLauncher>,
    entries: BTreeMap<String, Entry>,
    next_id: u64,
}

impl PreviewRegistry {
    pub fn new(launcher: Arc<dyn PreviewLauncher>) -> Self {
        Self {
            launcher,
            entries: BTreeMap::new(),
            next_id: 0,
        }
    }

    /// `quarto/previewStart`. `sink_for(id)` builds the event sink for a
    /// new session (it tags events with the session id).
    pub fn start(
        &mut self,
        id: RequestId,
        req: PreviewRequest,
        sink_for: impl FnOnce(String) -> PreviewEventSink,
    ) -> Vec<Message> {
        let key = self.launcher.session_key(&req);
        let mut out = Vec::new();
        let existing = self
            .entries
            .iter()
            .find(|(_, e)| e.key == key)
            .map(|(id, e)| (id.clone(), e.format == req.format));
        match existing {
            Some((preview_id, true)) => {
                let entry = self.entries.get_mut(&preview_id).expect("found above");
                match &mut entry.state {
                    State::Booting(waiters) => waiters.push((id, req.path)),
                    State::Ready { url } => {
                        let url = entry.session.url_for(&req.path).unwrap_or(url.clone());
                        out.push(started(id, &preview_id, &url, &entry.key));
                    }
                }
                return out;
            }
            // Same root, another format: the output directory is shared,
            // so the old session goes.
            Some((preview_id, false)) => out.extend(self.stop_entry(&preview_id, None)),
            None => {}
        }
        self.next_id += 1;
        let preview_id = format!("preview-{}", self.next_id);
        let format = req.format.clone();
        let path = req.path.clone();
        match self.launcher.start(req, sink_for(preview_id.clone())) {
            Ok(session) => {
                self.entries.insert(
                    preview_id,
                    Entry {
                        key,
                        format,
                        session,
                        state: State::Booting(vec![(id, path)]),
                    },
                );
            }
            Err(message) => out.push(error_response(id, &message)),
        }
        out
    }

    /// `quarto/previewStop`.
    pub fn stop(&mut self, id: RequestId, preview_id: &str) -> Vec<Message> {
        if !self.entries.contains_key(preview_id) {
            return vec![
                Response::new_err(
                    id,
                    ErrorCode::InvalidParams as i32,
                    format!("no preview {preview_id}"),
                )
                .into(),
            ];
        }
        let mut out = self.stop_entry(preview_id, None);
        out.push(Response::new_ok(id, Value::Null).into());
        out
    }

    /// An event from session `preview_id`.
    pub fn event(&mut self, preview_id: &str, event: PreviewEvent) -> Vec<Message> {
        let Some(entry) = self.entries.get_mut(preview_id) else {
            return Vec::new(); // a late event from a stopped session
        };
        match event {
            PreviewEvent::Ready { url, root: _ } => {
                let mut out = Vec::new();
                let state = std::mem::replace(&mut entry.state, State::Ready { url: url.clone() });
                if let State::Booting(waiters) = state {
                    for (id, path) in waiters {
                        let page = entry.session.url_for(&path).unwrap_or(url.clone());
                        out.push(started(id, preview_id, &page, &entry.key));
                    }
                }
                out.push(status(preview_id, json!({ "state": "ready", "url": url })));
                out
            }
            PreviewEvent::RenderStart => {
                vec![status(preview_id, json!({ "state": "rendering" }))]
            }
            PreviewEvent::RenderStop {
                ok,
                errors,
                warnings,
                text,
            } => vec![status(
                preview_id,
                json!({
                    "state": "rendered", "ok": ok, "errors": errors,
                    "warnings": warnings, "text": text,
                }),
            )],
            PreviewEvent::Ended { error } => self.stop_entry(preview_id, error),
        }
    }

    /// Stop every session (server shutdown).
    pub fn stop_all(&mut self) {
        let ids: Vec<String> = self.entries.keys().cloned().collect();
        for id in ids {
            let _ = self.stop_entry(&id, None);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn stop_entry(&mut self, preview_id: &str, error: Option<String>) -> Vec<Message> {
        let Some(entry) = self.entries.remove(preview_id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let State::Booting(waiters) = &entry.state {
            let message = error
                .clone()
                .unwrap_or_else(|| "the preview was stopped before it was ready".to_string());
            for (id, _) in waiters {
                out.push(error_response(id.clone(), &message));
            }
        }
        entry.session.stop();
        let mut params = json!({ "state": "stopped" });
        if let Some(message) = error {
            params["error"] = Value::String(message);
        }
        out.push(status(preview_id, params));
        out
    }
}

fn started(id: RequestId, preview_id: &str, url: &str, root: &Path) -> Message {
    Response::new_ok(
        id,
        json!({
            "previewId": preview_id,
            "url": url,
            "root": quarto_util::to_forward_slashes(root),
        }),
    )
    .into()
}

fn status(preview_id: &str, mut params: Value) -> Message {
    params["previewId"] = Value::String(preview_id.to_string());
    Notification::new(PREVIEW_STATUS.to_string(), params).into()
}

fn error_response(id: RequestId, message: &str) -> Message {
    Response::new_err(id, ErrorCode::RequestFailed as i32, message.to_string()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        started: Mutex<Vec<PreviewRequest>>,
        stopped: Arc<Mutex<Vec<PathBuf>>>,
        sinks: Mutex<Vec<PreviewEventSink>>,
        fail: bool,
    }

    struct FakeSession {
        root: PathBuf,
        stopped: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl PreviewSession for FakeSession {
        fn url_for(&self, path: &Path) -> Option<String> {
            let name = path.file_stem()?.to_string_lossy().into_owned();
            Some(format!("http://127.0.0.1:1/{name}.html"))
        }
        fn stop(self: Box<Self>) {
            self.stopped.lock().unwrap().push(self.root);
        }
    }

    impl PreviewLauncher for Fake {
        fn session_key(&self, req: &PreviewRequest) -> PathBuf {
            req.path.parent().unwrap().to_path_buf()
        }
        fn start(
            &self,
            req: PreviewRequest,
            events: PreviewEventSink,
        ) -> Result<Box<dyn PreviewSession>, String> {
            if self.fail {
                return Err("cannot preview".into());
            }
            let root = self.session_key(&req);
            self.started.lock().unwrap().push(req);
            self.sinks.lock().unwrap().push(events);
            Ok(Box::new(FakeSession {
                root,
                stopped: self.stopped.clone(),
            }))
        }
    }

    fn req(path: &str) -> PreviewRequest {
        PreviewRequest {
            path: PathBuf::from(path),
            format: None,
        }
    }

    fn noop_sink(_: String) -> PreviewEventSink {
        Box::new(|_| {})
    }

    fn responses(messages: &[Message]) -> Vec<(RequestId, Result<Value, String>)> {
        messages
            .iter()
            .filter_map(|m| match m {
                Message::Response(r) => Some((
                    r.id.clone(),
                    r.response_result.clone().map_err(|e| e.message),
                )),
                _ => None,
            })
            .collect()
    }

    fn statuses(messages: &[Message]) -> Vec<String> {
        messages
            .iter()
            .filter_map(|m| match m {
                Message::Notification(n) if n.method == PREVIEW_STATUS => {
                    Some(n.params["state"].as_str().unwrap().to_string())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_start_is_answered_when_the_session_is_ready() {
        let fake = Arc::new(Fake::default());
        let mut reg = PreviewRegistry::new(fake.clone());
        assert!(reg.start(1.into(), req("/p/a.qmd"), noop_sink).is_empty());
        let out = reg.event(
            "preview-1",
            PreviewEvent::Ready {
                url: "http://127.0.0.1:1/a.html".into(),
                root: "/p".into(),
            },
        );
        let r = responses(&out);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].1.as_ref().unwrap()["previewId"], "preview-1");
        assert_eq!(statuses(&out), ["ready"]);
    }

    #[test]
    fn documents_with_the_same_root_share_one_session() {
        let fake = Arc::new(Fake::default());
        let mut reg = PreviewRegistry::new(fake.clone());
        reg.start(1.into(), req("/p/a.qmd"), noop_sink);
        // While booting: queued, answered with its own page.
        assert!(reg.start(2.into(), req("/p/b.qmd"), noop_sink).is_empty());
        let out = reg.event(
            "preview-1",
            PreviewEvent::Ready {
                url: "u".into(),
                root: "/p".into(),
            },
        );
        let urls: Vec<_> = responses(&out)
            .into_iter()
            .map(|(_, v)| v.unwrap()["url"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            urls,
            ["http://127.0.0.1:1/a.html", "http://127.0.0.1:1/b.html"]
        );
        // Once ready: answered at once.
        let out = reg.start(3.into(), req("/p/c.qmd"), noop_sink);
        assert_eq!(responses(&out).len(), 1);
        assert_eq!(fake.started.lock().unwrap().len(), 1);
        // Another root gets its own session.
        reg.start(4.into(), req("/q/a.qmd"), noop_sink);
        assert_eq!(fake.started.lock().unwrap().len(), 2);
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn another_format_replaces_the_session_for_that_root() {
        let fake = Arc::new(Fake::default());
        let mut reg = PreviewRegistry::new(fake.clone());
        reg.start(1.into(), req("/p/a.qmd"), noop_sink);
        let mut typst = req("/p/a.qmd");
        typst.format = Some("typst".into());
        let out = reg.start(2.into(), typst, noop_sink);
        // The first request is failed, the old session stopped.
        assert!(responses(&out)[0].1.is_err());
        assert_eq!(*fake.stopped.lock().unwrap(), [PathBuf::from("/p")]);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn a_session_that_ends_while_booting_fails_its_requests() {
        let fake = Arc::new(Fake::default());
        let mut reg = PreviewRegistry::new(fake);
        reg.start(1.into(), req("/p/a.qmd"), noop_sink);
        let out = reg.event(
            "preview-1",
            PreviewEvent::Ended {
                error: Some("epub cannot be previewed".into()),
            },
        );
        assert_eq!(
            responses(&out)[0].1.as_ref().unwrap_err(),
            "epub cannot be previewed"
        );
        assert_eq!(statuses(&out), ["stopped"]);
        assert!(reg.is_empty());
    }

    #[test]
    fn stop_stops_the_session_and_unknown_ids_are_errors() {
        let fake = Arc::new(Fake::default());
        let mut reg = PreviewRegistry::new(fake.clone());
        reg.start(1.into(), req("/p/a.qmd"), noop_sink);
        reg.event(
            "preview-1",
            PreviewEvent::Ready {
                url: "u".into(),
                root: "/p".into(),
            },
        );
        let out = reg.stop(2.into(), "preview-1");
        assert!(responses(&out)[0].1.is_ok());
        assert_eq!(statuses(&out), ["stopped"]);
        assert_eq!(fake.stopped.lock().unwrap().len(), 1);
        assert!(responses(&reg.stop(3.into(), "preview-1"))[0].1.is_err());
        // A late event from the stopped session is ignored.
        assert!(reg.event("preview-1", PreviewEvent::RenderStart).is_empty());
    }

    #[test]
    fn a_launcher_error_fails_the_request() {
        let fake = Arc::new(Fake {
            fail: true,
            ..Fake::default()
        });
        let mut reg = PreviewRegistry::new(fake);
        let out = reg.start(1.into(), req("/p/a.qmd"), noop_sink);
        assert_eq!(responses(&out)[0].1.as_ref().unwrap_err(), "cannot preview");
        assert!(reg.is_empty());
    }

    #[test]
    fn render_events_become_status_notifications() {
        let fake = Arc::new(Fake::default());
        let mut reg = PreviewRegistry::new(fake);
        reg.start(1.into(), req("/p/a.qmd"), noop_sink);
        assert_eq!(
            statuses(&reg.event("preview-1", PreviewEvent::RenderStart)),
            ["rendering"]
        );
        let out = reg.event(
            "preview-1",
            PreviewEvent::RenderStop {
                ok: false,
                errors: 2,
                warnings: 0,
                text: "bad".into(),
            },
        );
        let Message::Notification(n) = &out[0] else {
            panic!()
        };
        assert_eq!(n.params["errors"], 2);
        assert_eq!(n.params["previewId"], "preview-1");
    }
}
