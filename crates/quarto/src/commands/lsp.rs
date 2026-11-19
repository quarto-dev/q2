//! `q2 lsp`: the language server, with in-process static previews.
//!
//! `quarto-lsp` owns the protocol and the preview registry. It cannot
//! depend on this crate, where the render and the static-preview session
//! live, so this module implements its [`PreviewLauncher`] on top of
//! [`run_session`] and hands it to the server (bd-foum4ql3;
//! plan `claude-notes/plans/2026-11-17-q2-vscode-extension.md`, D-E).
//!
//! Sessions are tasks on one multi-threaded tokio runtime, created the
//! first time a preview starts. The server loop itself stays synchronous;
//! session events reach it through the registry's event sink.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::Result;
use quarto_lsp::preview::{
    HostServices, PreviewEvent, PreviewEventSink, PreviewLauncher, PreviewRequest, PreviewSession,
};
use quarto_preview::static_mode::ReloadEvent;

use super::preview_static::{
    EventSink, PageIndex, SessionEvent, StaticArgs, run_session, session_root,
};

/// Execute the LSP server over stdio, with previews.
pub fn execute() -> Result<()> {
    let runtime: Arc<OnceLock<tokio::runtime::Runtime>> = Arc::default();
    let launcher = Arc::new(StaticPreviewLauncher {
        runtime: runtime.clone(),
    });
    let result = quarto_lsp::server::run_server_with(HostServices {
        previews: Some(launcher),
    });
    // Sessions were asked to stop when the server exited. A render still
    // running on a blocking thread is not waited for past this.
    if let Some(runtime) = Arc::into_inner(runtime).and_then(OnceLock::into_inner) {
        runtime.shutdown_timeout(Duration::from_secs(5));
    }
    result
}

struct StaticPreviewLauncher {
    runtime: Arc<OnceLock<tokio::runtime::Runtime>>,
}

impl StaticPreviewLauncher {
    fn runtime(&self) -> Result<&tokio::runtime::Runtime, String> {
        if let Some(rt) = self.runtime.get() {
            return Ok(rt);
        }
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("q2-preview")
            .build()
            .map_err(|e| format!("starting the preview runtime: {e}"))?;
        Ok(self.runtime.get_or_init(|| rt))
    }
}

fn canonical(path: &Path) -> PathBuf {
    quarto_system_runtime::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl PreviewLauncher for StaticPreviewLauncher {
    fn session_key(&self, req: &PreviewRequest) -> PathBuf {
        session_root(&canonical(&req.path), &[])
    }

    fn start(
        &self,
        req: PreviewRequest,
        events: PreviewEventSink,
    ) -> Result<Box<dyn PreviewSession>, String> {
        let runtime = self.runtime()?;
        let events: Arc<PreviewEventSink> = Arc::new(events);
        let base: Arc<OnceLock<(String, PageIndex)>> = Arc::default();

        let sink_events = events.clone();
        let sink_base = base.clone();
        let sink: EventSink = Arc::new(move |event| match event {
            SessionEvent::Ready(ready) => {
                let _ =
                    sink_base.set((format!("http://{}:{}", ready.host, ready.port), ready.pages));
                sink_events(PreviewEvent::Ready {
                    url: ready.url,
                    root: ready.input,
                });
            }
            SessionEvent::Render(ReloadEvent::RenderStart) => {
                sink_events(PreviewEvent::RenderStart);
            }
            SessionEvent::Render(ReloadEvent::RenderStop {
                ok,
                errors,
                warnings,
                text,
            }) => sink_events(PreviewEvent::RenderStop {
                ok,
                errors,
                warnings,
                text,
            }),
            // The page reloads itself over SSE.
            SessionEvent::Render(ReloadEvent::Reload { .. }) => {}
        });

        let args = StaticArgs {
            path: Some(req.path),
            port: Some(0),
            host: None,
            no_browser: true,
            browser: None,
            no_watch: false,
            no_navigate: false,
            to: req.format,
            metadata: Vec::new(),
            profile: Vec::new(),
        };
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        runtime.spawn(async move {
            let stop = async move {
                let _ = stop_rx.await;
            };
            let error = run_session(args, sink, stop)
                .await
                .err()
                .map(|e| e.plain_message());
            // After a requested stop the registry has already dropped the
            // session and ignores this.
            events(PreviewEvent::Ended { error });
        });
        Ok(Box::new(StaticSession {
            stop: Some(stop_tx),
            base,
            // Re-renders reuse warm Jupyter kernels for the session's life,
            // as in `q2 preview --static` (bd-hxhnnlzs). Dropped on the
            // server thread, outside the runtime.
            _kernels: quarto_core::engine::jupyter::kernel_scope(),
        }))
    }
}

struct StaticSession {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    /// `http://HOST:PORT` and the page index, once ready.
    base: Arc<OnceLock<(String, PageIndex)>>,
    _kernels: quarto_core::engine::jupyter::KernelScope,
}

impl PreviewSession for StaticSession {
    fn url_for(&self, path: &Path) -> Option<String> {
        let (base, pages) = self.base.get()?;
        Some(format!("{base}{}", pages.page_of(&canonical(path))?))
    }

    fn stop(mut self: Box<Self>) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
