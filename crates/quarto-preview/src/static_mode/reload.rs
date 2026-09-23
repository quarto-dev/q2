//! The reload channel for `q2 preview --static` (bd-sl79jjiq, plan
//! § Reload channel): a broadcast of [`ReloadEvent`]s from the render
//! loop, delivered to every open page as server-sent events.
//!
//! SSE rather than a WebSocket because the channel is one-directional
//! and `EventSource` reconnects on its own, so a restarted server picks
//! its tabs back up without any client-side retry logic.

use axum::response::sse::Event;
use serde::Serialize;
use tokio::sync::{broadcast, watch};

/// What the render loop tells the browser. Serialized as the SSE
/// `data:` line (JSON, tagged by `type`); the SSE `event:` name is
/// [`Self::name`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ReloadEvent {
    /// A render started; the client shows its badge.
    RenderStart,
    /// A render finished. `text` is the plain-text diagnostics
    /// (`RenderReport::diagnostics_text(false)`), shown in the client's
    /// panel when `ok` is false.
    RenderStop {
        ok: bool,
        errors: usize,
        warnings: usize,
        text: String,
    },
    /// Outputs changed on disk. With a `target` (a root-relative URL
    /// such as `/posts/foo.html`) the client navigates there; without
    /// one it reloads in place.
    Reload { target: Option<String> },
}

impl ReloadEvent {
    /// The SSE event name the client subscribes to.
    pub fn name(&self) -> &'static str {
        match self {
            ReloadEvent::RenderStart => "render-start",
            ReloadEvent::RenderStop { .. } => "render-stop",
            ReloadEvent::Reload { .. } => "reload",
        }
    }

    /// The SSE frame for this event.
    pub(super) fn to_sse(&self) -> Event {
        Event::default()
            .event(self.name())
            .data(serde_json::to_string(self).expect("ReloadEvent has no unserializable field"))
    }
}

/// How many events a slow subscriber may fall behind before it starts
/// losing them. Events are cheap and reload-shaped, so a lagging tab
/// simply reloads on the next one it does receive.
const CHANNEL_CAPACITY: usize = 64;

/// The broadcast side of the channel. Cloned into the router state;
/// the render loop keeps one to `send` on.
#[derive(Clone, Debug)]
pub struct ReloadHub {
    tx: broadcast::Sender<ReloadEvent>,
    /// Flips to `true` once on [`Self::shutdown`]; every open SSE stream
    /// ends when it does, which is what lets the server's graceful
    /// shutdown finish (an `EventSource` connection never closes on
    /// its own).
    closing: watch::Sender<bool>,
}

impl Default for ReloadHub {
    fn default() -> Self {
        Self::new()
    }
}

impl ReloadHub {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(CHANNEL_CAPACITY);
        let (closing, _) = watch::channel(false);
        Self { tx, closing }
    }

    /// End every open SSE stream. Idempotent.
    pub fn shutdown(&self) {
        let _ = self.closing.send(true);
    }

    /// A stream of the closing flag: its current value first, then
    /// every change. Yields `true` once [`Self::shutdown`] runs.
    pub(super) fn closing_stream(&self) -> tokio_stream::wrappers::WatchStream<bool> {
        tokio_stream::wrappers::WatchStream::new(self.closing.subscribe())
    }

    /// Broadcast to every current subscriber. Returns how many there
    /// were; zero (nobody listening) is not an error.
    pub fn send(&self, event: ReloadEvent) -> usize {
        self.tx.send(event).unwrap_or(0)
    }

    /// A receiver that sees only events sent from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<ReloadEvent> {
        self.tx.subscribe()
    }

    /// Open subscribers right now (open SSE connections).
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialize_with_a_type_tag_and_kebab_case_names() {
        let start = ReloadEvent::RenderStart;
        assert_eq!(start.name(), "render-start");
        assert_eq!(
            serde_json::to_string(&start).unwrap(),
            r#"{"type":"render-start"}"#
        );

        let stop = ReloadEvent::RenderStop {
            ok: true,
            errors: 0,
            warnings: 3,
            text: String::new(),
        };
        assert_eq!(stop.name(), "render-stop");
        assert_eq!(
            serde_json::to_string(&stop).unwrap(),
            r#"{"type":"render-stop","ok":true,"errors":0,"warnings":3,"text":""}"#
        );

        let reload = ReloadEvent::Reload { target: None };
        assert_eq!(reload.name(), "reload");
        assert_eq!(
            serde_json::to_string(&reload).unwrap(),
            r#"{"type":"reload","target":null}"#
        );
    }

    #[test]
    fn send_reports_the_live_subscriber_count() {
        let hub = ReloadHub::new();
        assert_eq!(hub.subscriber_count(), 0);
        assert_eq!(hub.send(ReloadEvent::RenderStart), 0);
        let mut rx = hub.subscribe();
        assert_eq!(hub.subscriber_count(), 1);
        assert_eq!(hub.send(ReloadEvent::Reload { target: None }), 1);
        assert_eq!(
            rx.try_recv().unwrap(),
            ReloadEvent::Reload { target: None },
            "a subscriber sees only what was sent after it subscribed"
        );
        assert!(rx.try_recv().is_err(), "nothing else queued");
    }
}
