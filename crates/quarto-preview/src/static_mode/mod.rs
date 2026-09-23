//! `q2 preview --static` (bd-sl79jjiq): the library half of the
//! Quarto-1-style preview — a static-file router over a rendered
//! output directory, an SSE reload channel injected into every served
//! HTML page, and the pure policy that turns a filesystem change into
//! "ignore / re-render this input / re-render everything".
//!
//! Nothing here touches the hub, samod, or the embedded SPA. The
//! render loop that drives it lives in the `q2` binary
//! (`crates/quarto/src/commands/preview_static.rs`) because rendering
//! is dispatched there. Plan:
//! `claude-notes/plans/2026-09-22-q2-preview-static.md`.

pub mod reload;
pub mod server;
pub mod watch_policy;

pub use reload::{ReloadEvent, ReloadHub};
pub use server::{EVENTS_PATH, StaticServerConfig, build_router, serve};
pub use watch_policy::{
    Action, ContentTracker, WatchContext, classify, is_config_like, is_input_extension,
};
