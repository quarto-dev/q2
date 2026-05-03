//! `PublishRenderer` — the thing a provider calls to get bytes to
//! upload.
//!
//! The trait is **deliberately narrow**:
//!
//! - It does not expose `ProjectContext`, `ProjectPipeline`, or
//!   anything else internal to `quarto-core`. A WASM-side
//!   implementation drives the in-browser pipeline; a native
//!   implementation drives `ProjectPipeline`. Both produce
//!   `PublishFiles` via the same call.
//! - The result is a `PublishFiles` derived from the renderer's
//!   own knowledge of what it produced — *not* a filesystem walk.
//!   This is what makes the contract WASM-portable (a browser
//!   implementation has no filesystem to walk).
//! - Render flags are a tiny, stable shape the provider populates
//!   (e.g. `site_url`). Anything richer should be configured on the
//!   renderer itself, not pushed across the trait.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::types::{PublishError, PublishFiles};

/// Render-time hints a provider can pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublishRenderFlags {
    /// Override `website.site-url` for this render. Providers that
    /// host content at a known URL (Quarto Pub, Netlify with a
    /// stable site name) set this so generated `<link
    /// rel=canonical>` and sitemaps point at the live URL.
    pub site_url: Option<String>,
}

/// Source of bytes to publish.
#[async_trait]
pub trait PublishRenderer: Send + Sync {
    /// Render and return the file list to publish. Implementations
    /// are responsible for noting which files came from the render
    /// vs. were already on disk — *only the renderer-known set
    /// should be returned* (see crate docs).
    async fn render(&self, flags: &PublishRenderFlags) -> Result<PublishFiles, PublishError>;
}
