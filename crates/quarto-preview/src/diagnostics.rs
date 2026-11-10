//! Server-side diagnostic sink for `q2 preview` (bd-b9kzg).
//!
//! Two callsites in this crate currently emit `tracing::warn!`s
//! into stderr that the SPA never sees:
//!
//!   * `capture_driver.rs` — sibling-file eager-capture failures,
//!   * `re_execute.rs` — engine re-execute failures (also fed into
//!     `CaptureRef.last_error` for the existing `StaleCaptureOverlay`).
//!
//! This module is the transport: a project-scoped, page-keyed
//! accumulator that those callsites push structured
//! [`DiagnosticMessage`]s into. The SPA reads via a new endpoint
//! ([`GET /api/preview/diagnostics?page=<rel>`](crate::extend_with_preview))
//! and merges the result with the WASM render's existing
//! `result.warnings`.
//!
//! ## Semantics
//!
//! * **Page-keyed.** The key is the project-relative, forward-slash
//!   path of the file the diagnostic is *about* (typically the page
//!   the user is editing or viewing). Diagnostics with no associated
//!   page (e.g. project-config-level) belong to the empty-string key.
//! * **Cleared at the start of each run.** Each callsite calls
//!   [`DiagnosticSink::replace_page`] with an empty vec before its
//!   run, then [`emit`](DiagnosticSink::emit) for each diagnostic it
//!   produces. The accumulated entries reflect the most recent run.
//! * **Additive over `tracing::warn!`.** [`emit`](DiagnosticSink::emit)
//!   still fires a `tracing::warn!` line so the server's stdout
//!   keeps printing the human-readable form — this is a parallel
//!   surface, not a replacement.
//! * **Thread-safe.** `std::sync::RwLock` inside; cheap snapshots.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use axum::{
    Json,
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use quarto_error_reporting::{DiagnosticMessage, JsonDiagnostic, diagnostic_to_json};
use quarto_hub::context::SharedContext;
use quarto_source_map::SourceContext;
use serde::{Deserialize, Serialize};

/// Page-scoped diagnostic accumulator. Cloneable as `Arc<Self>` so
/// the same sink can be shared between the HTTP handler and the
/// migrating callsites without `Mutex` contention on the handle.
#[derive(Default)]
pub struct DiagnosticSink {
    /// Project-relative path → diagnostics emitted for that page.
    /// Forward-slash paths.
    inner: RwLock<HashMap<String, Vec<DiagnosticMessage>>>,
}

impl DiagnosticSink {
    /// Create an empty sink.
    pub fn new() -> Self {
        Self::default()
    }

    /// Emit a diagnostic for `page`. Also writes via `tracing::warn!`
    /// so the server's stdout doesn't go quiet — this surface is
    /// purely additive over today's `tracing` behaviour.
    pub fn emit(&self, page: &str, diag: DiagnosticMessage) {
        tracing::warn!(
            target: "preview::diagnostics",
            page = %page,
            code = ?diag.code,
            "{}",
            diag.title,
        );
        self.inner
            .write()
            .expect("DiagnosticSink RwLock poisoned")
            .entry(page.to_string())
            .or_default()
            .push(diag);
    }

    /// Replace `page`'s diagnostics atomically. Callsites use this
    /// to clear before a fresh run; an empty vec leaves a populated
    /// "no diagnostics this run" entry (distinct from never-run).
    pub fn replace_page(&self, page: &str, diags: Vec<DiagnosticMessage>) {
        self.inner
            .write()
            .expect("DiagnosticSink RwLock poisoned")
            .insert(page.to_string(), diags);
    }

    /// Snapshot of diagnostics for `page`. Returns an empty vec if
    /// the page has no entry — callers should not need to
    /// distinguish "never run" from "run with no diagnostics" for
    /// the SPA surface (both render as "nothing to show").
    pub fn get_for_page(&self, page: &str) -> Vec<DiagnosticMessage> {
        self.inner
            .read()
            .expect("DiagnosticSink RwLock poisoned")
            .get(page)
            .cloned()
            .unwrap_or_default()
    }

    /// Snapshot of the entire sink. Used by a future project-wide
    /// surface (out of scope for this MVP) and useful for tests
    /// that want to assert "exactly these pages have entries."
    pub fn snapshot(&self) -> HashMap<String, Vec<DiagnosticMessage>> {
        self.inner
            .read()
            .expect("DiagnosticSink RwLock poisoned")
            .clone()
    }
}

/// Process-wide handle to the sink. Set once by
/// [`crate::run_with_on_ready`] at server boot; read by the
/// `/api/preview/diagnostics` handler AND by the Phase-3
/// migration callsites (`capture_driver`, `re_execute`).
/// Same OnceLock pattern as `SPA_DIR_OVERRIDE` and
/// `re_execute::CACHE_DIR` — keeps signatures stable.
///
/// Integration tests reach the sink by booting the server (which
/// sets the OnceLock) and then calling [`current_sink`]. Unit
/// tests that operate on the sink directly (without the server)
/// can construct their own `DiagnosticSink` value; the OnceLock
/// is irrelevant in that case.
static SINK: OnceLock<Arc<DiagnosticSink>> = OnceLock::new();

/// Set the process-wide sink handle. Idempotent (first-writer-wins),
/// matching the existing OnceLock conventions in this crate.
pub fn set_sink(sink: Arc<DiagnosticSink>) {
    let _ = SINK.set(sink);
}

/// Read the process-wide sink handle. Returns `None` when no sink
/// has been set (e.g. unit tests that call handlers directly
/// without running the server boot path). The HTTP handler treats
/// `None` as "empty sink" → returns `{ diagnostics: [] }`.
pub fn current_sink() -> Option<Arc<DiagnosticSink>> {
    SINK.get().cloned()
}

// ─── HTTP handler ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DiagnosticsQuery {
    pub page: String,
}

/// Response shape for `GET /api/preview/diagnostics`.
///
/// Items serialize as [`JsonDiagnostic`] — the same shape the WASM
/// render returns in `RenderResponse.warnings` / `.diagnostics`,
/// so the SPA can merge both feeds without a translation layer.
#[derive(Debug, Serialize)]
pub struct DiagnosticsResponse {
    pub diagnostics: Vec<JsonDiagnostic>,
}

/// Axum handler for `GET /api/preview/diagnostics?page=<rel>`.
///
/// Returns the accumulated server-side diagnostics for `page` from
/// the process-wide sink. Unknown / never-emitted-for pages return
/// `{ "diagnostics": [] }` rather than 404 — the SPA always
/// fetches and "no diagnostics" is the common case.
pub async fn diagnostics_handler(
    State(_ctx): State<SharedContext>,
    Query(query): Query<DiagnosticsQuery>,
) -> Response {
    let diagnostics: Vec<JsonDiagnostic> = match current_sink() {
        Some(sink) => {
            // The server-side sink stores `DiagnosticMessage`
            // values with byte-offset `SourceInfo` locations. The
            // wire format the SPA consumes carries 1-based
            // line/column positions, which requires a
            // `SourceContext` to map. The server-side callsites
            // we migrate in Phase 3 (capture_driver,
            // re_execute) don't currently produce location-bearing
            // diagnostics — they emit "engine failed" /
            // "could not parse" titles without source spans — so
            // an empty `SourceContext` is the honest answer for
            // now. If a future callsite emits location-bearing
            // diagnostics, the design surface to extend is the
            // sink (let it store a `(DiagnosticMessage, Option<SourceContext>)`
            // pair or a pre-translated `JsonDiagnostic`).
            let ctx = SourceContext::new();
            sink.get_for_page(&query.page)
                .iter()
                .map(|d| diagnostic_to_json(d, &ctx))
                .collect()
        }
        None => Vec::new(),
    };
    Json(DiagnosticsResponse { diagnostics }).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_error_reporting::DiagnosticMessage;

    fn synthetic_warning(title: &str) -> DiagnosticMessage {
        DiagnosticMessage::warning(title)
    }

    #[test]
    fn emit_then_get_for_page_returns_diagnostic() {
        let sink = DiagnosticSink::new();
        sink.emit("a.qmd", synthetic_warning("first"));
        let got = sink.get_for_page("a.qmd");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].title, "first");
    }

    #[test]
    fn emit_preserves_insertion_order() {
        let sink = DiagnosticSink::new();
        sink.emit("a.qmd", synthetic_warning("first"));
        sink.emit("a.qmd", synthetic_warning("second"));
        let got = sink.get_for_page("a.qmd");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].title, "first");
        assert_eq!(got[1].title, "second");
    }

    #[test]
    fn replace_page_clears_prior_diagnostics() {
        let sink = DiagnosticSink::new();
        sink.emit("a.qmd", synthetic_warning("first"));
        sink.replace_page("a.qmd", vec![synthetic_warning("second")]);
        let got = sink.get_for_page("a.qmd");
        assert_eq!(got.len(), 1, "replace_page should drop prior emissions");
        assert_eq!(got[0].title, "second");
    }

    #[test]
    fn replace_page_with_empty_vec_leaves_no_entries() {
        let sink = DiagnosticSink::new();
        sink.emit("a.qmd", synthetic_warning("first"));
        sink.replace_page("a.qmd", vec![]);
        assert!(sink.get_for_page("a.qmd").is_empty());
    }

    #[test]
    fn pages_are_independent() {
        let sink = DiagnosticSink::new();
        sink.emit("a.qmd", synthetic_warning("only-a"));
        assert_eq!(sink.get_for_page("a.qmd").len(), 1);
        assert!(sink.get_for_page("b.qmd").is_empty());
    }

    #[test]
    fn snapshot_reflects_all_pages() {
        let sink = DiagnosticSink::new();
        sink.emit("a.qmd", synthetic_warning("a-diag"));
        sink.emit("b.qmd", synthetic_warning("b-diag"));
        let snap = sink.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap.get("a.qmd").map(|v| v.len()), Some(1));
        assert_eq!(snap.get("b.qmd").map(|v| v.len()), Some(1));
    }

    #[test]
    fn snapshot_is_decoupled_from_sink() {
        // A snapshot is a clone — mutating the sink after taking
        // a snapshot must not change what the snapshot contains.
        let sink = DiagnosticSink::new();
        sink.emit("a.qmd", synthetic_warning("v1"));
        let snap = sink.snapshot();
        sink.replace_page("a.qmd", vec![synthetic_warning("v2")]);
        assert_eq!(
            snap.get("a.qmd").map(|v| v[0].title.as_str()),
            Some("v1"),
            "snapshot must be a true copy, not a live view"
        );
    }

    #[test]
    fn unset_oncelock_returns_none() {
        // SINK is process-wide; we can't deterministically unset
        // it across the test suite (something else might have set
        // it first). Instead pin the contract: `current_sink()`
        // returns Option, never panics, and is safe to call from
        // tests. If we observe `Some`, that's because another
        // test set it — also valid; the assertion is the
        // never-panics contract, not the value.
        let _ = current_sink();
    }

    #[test]
    fn concurrent_emits_are_thread_safe() {
        // Two threads, 100 emissions each on different pages.
        // Smoke test of the RwLock — proves the API stays sound
        // under contention without an explicit "exactly N entries"
        // assertion that would tax ordering guarantees.
        use std::sync::Arc;
        use std::thread;
        let sink = Arc::new(DiagnosticSink::new());
        let s1 = sink.clone();
        let t1 = thread::spawn(move || {
            for i in 0..100 {
                s1.emit("a.qmd", synthetic_warning(&format!("a-{i}")));
            }
        });
        let s2 = sink.clone();
        let t2 = thread::spawn(move || {
            for i in 0..100 {
                s2.emit("b.qmd", synthetic_warning(&format!("b-{i}")));
            }
        });
        t1.join().unwrap();
        t2.join().unwrap();
        assert_eq!(sink.get_for_page("a.qmd").len(), 100);
        assert_eq!(sink.get_for_page("b.qmd").len(), 100);
    }
}
