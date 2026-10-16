//! `POST /api/preview/pdf`: the inline, supersede-aware PDF for a typst
//! document's preview pane (epic E1, Plan 9).
//!
//! Body `{ path, content?, metadata?, attributionJson? }`, the shared contract of
//! [`crate::render_helper`]: the response is `application/pdf` without
//! `Content-Disposition`. The render is `typst-pdf` (E8: a PDF whatever
//! `output-ext` says).
//!
//! **Latest wins, per path.** While a render of a path runs, a newer request
//! for it replaces the older one. The newer request waits (asynchronously,
//! on a [`Notify`], never on a blocking-pool thread) for the running render
//! to finish; the running one is cancelled (Plan 1's runner kills its child)
//! and its result is discarded with the superseded 409
//! `{"superseded": true}`. A request replaced while it waits gets the same
//! 409 at once. At most one render per path runs, and only the winner calls
//! `spawn_blocking`.
//!
//! The slot is owned by the handler until `spawn_blocking`, then moved into
//! the closure: it is released when the blocking task finishes (the request
//! directory is gone by then), not when a client disconnect drops the handler
//! future. Several tabs on one path therefore coalesce.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use quarto_core::pandoc_request::formats::TYPST_PDF_KEY;
use quarto_core::stage::Cancellation;
use quarto_hub::context::SharedContext;
use serde::Deserialize;
use tokio::sync::Notify;

use crate::render_helper::{
    CancelOnDrop, RenderJob, RenderOutcome, locate, may_render_book, note_stale_scope,
    outcome_response, render_blocking, request_metadata, resolve_all_captures, resolve_captures,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfRequest {
    pub path: String,
    /// The editor's text; the disk copy is rendered when absent (an explicit
    /// empty string is an empty document).
    #[serde(default)]
    pub content: Option<String>,
    /// Runtime-layer metadata for this render (a JSON object); the PDF pane
    /// sends `{"book-scope": "chapter"}`.
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub attribution_json: Option<String>,
    /// Fields no longer (or not yet) read: kept to log a stale client's `scope`.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Default)]
struct PathSlot {
    /// The newest ticket taken for this path.
    latest: u64,
    /// A blocking render holds the slot.
    busy: bool,
    /// Requests parked in [`acquire`].
    waiting: usize,
    /// The running render's cancellation, set when a newer request arrives.
    running: Option<Cancellation>,
}

struct Slots {
    map: Mutex<HashMap<String, PathSlot>>,
    /// Woken whenever a slot's state changes; waiters recheck their own path.
    changed: Notify,
}

static SLOTS: LazyLock<Slots> = LazyLock::new(|| Slots {
    map: Mutex::new(HashMap::new()),
    changed: Notify::new(),
});

static TICKET: AtomicU64 = AtomicU64::new(0);

fn lock() -> std::sync::MutexGuard<'static, HashMap<String, PathSlot>> {
    SLOTS.map.lock().unwrap_or_else(|e| e.into_inner())
}

/// Holds a path's slot; dropping it frees the slot for the next request.
struct SlotGuard {
    key: String,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        {
            let mut map = lock();
            if let Some(slot) = map.get_mut(&self.key) {
                slot.busy = false;
                slot.running = None;
                if slot.waiting == 0 {
                    map.remove(&self.key);
                }
            }
        }
        SLOTS.changed.notify_waiters();
    }
}

/// Counts a request as waiting; the last one out of an idle slot removes it.
struct WaitGuard<'a> {
    key: &'a str,
}

impl Drop for WaitGuard<'_> {
    fn drop(&mut self) {
        let mut map = lock();
        if let Some(slot) = map.get_mut(self.key) {
            slot.waiting -= 1;
            if slot.waiting == 0 && !slot.busy {
                map.remove(self.key);
            }
        }
    }
}

enum Acquired {
    Won(SlotGuard),
    Superseded,
}

/// Take a ticket for `key`, cancel the render that is running, and wait for
/// the slot. A newer ticket for the same key ends the wait as superseded.
async fn acquire(key: &str, cancel: &Cancellation) -> Acquired {
    let ticket = TICKET.fetch_add(1, Ordering::Relaxed) + 1;
    {
        let mut map = lock();
        let slot = map.entry(key.to_string()).or_default();
        slot.latest = ticket;
        slot.waiting += 1;
        if let Some(running) = &slot.running {
            running.cancel();
        }
    }
    let _waiting = WaitGuard { key };
    // Older waiters notice the new ticket now rather than when the running
    // render ends.
    SLOTS.changed.notify_waiters();
    loop {
        // Register for the wake-up before looking at the state, so a change
        // between the look and the await is not missed.
        let notified = SLOTS.changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        {
            let mut map = lock();
            let slot = map.get_mut(key).expect("a waiting request keeps its slot");
            if slot.latest != ticket {
                return Acquired::Superseded;
            }
            if !slot.busy {
                slot.busy = true;
                slot.running = Some(cancel.clone());
                return Acquired::Won(SlotGuard {
                    key: key.to_string(),
                });
            }
        }
        notified.await;
    }
}

/// Axum handler for `POST /api/preview/pdf`.
pub async fn pdf_handler(
    State(ctx): State<SharedContext>,
    Json(body): Json<PdfRequest>,
) -> Response {
    let (abs_path, hub_root) = match locate(&ctx, &body.path) {
        Ok(located) => located,
        Err(response) => return response,
    };
    note_stale_scope(&body.extra, "pdf");
    let metadata = match request_metadata(body.metadata) {
        Ok(m) => m,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };

    let cancel = Cancellation::new();
    let _client_gone = CancelOnDrop(cancel.clone());
    let slot = match acquire(&body.path, &cancel).await {
        Acquired::Won(slot) => slot,
        Acquired::Superseded => return superseded(),
    };
    // Captures come from the sidecar entry of this path only (E10).
    let captures = resolve_captures(&ctx, &body.path).await;
    let all_captures = if may_render_book(TYPST_PDF_KEY, metadata.as_ref()) {
        resolve_all_captures(&ctx).await
    } else {
        Default::default()
    };
    if cancel.is_cancelled() {
        return superseded();
    }
    let job = RenderJob {
        abs_path,
        format_key: TYPST_PDF_KEY.to_string(),
        content: body.content,
        metadata,
        attribution_json: body.attribution_json,
        captures,
        all_captures,
        hub_root,
        cancellation: cancel.clone(),
    };
    let outcome = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        render_blocking(job)
    })
    .await;
    match outcome {
        // A newer request cancelled this one while it ran: its result is stale.
        Ok(_) if cancel.is_cancelled() => superseded(),
        Ok(outcome) => outcome_response(outcome, "application/pdf", false),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("render task failed: {e}"),
        )
            .into_response(),
    }
}

fn superseded() -> Response {
    outcome_response(RenderOutcome::Superseded, "application/pdf", false)
}
