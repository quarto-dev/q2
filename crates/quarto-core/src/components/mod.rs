/*
 * components/mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Components bound to document elements (bd-react-components-a8flgldm).
 */

//! Components bound to document elements.
//!
//! A document declares components in metadata:
//!
//! ```yaml
//! components:
//!   - name: carousel
//!     src: carousel.js        # document-relative; a leading / is the project root
//!     bind: "#carousel"       # "#id" or ".class"; default "#<name>"
//!     content: dom            # dom | ast | none
//! ```
//!
//! and the [`ComponentsResolveTransform`](crate::transforms::ComponentsResolveTransform)
//! marks every bound `Div`/`Span` with `data-quarto-component="<name>"`,
//! copies the component modules under the project lib dir, registers the
//! vendored React ES modules and the client runtime as artifacts, and
//! puts an import map, a manifest and the runtime's module script in the
//! page head. The static markup is untouched: it is the no-script
//! version, and the runtime mounts the component over it in the browser.
//!
//! - [`config`]: the `components:` metadata contract and its diagnostics.
//! - [`runtime`]: the embedded assets, their artifact keys and the head
//!   block.
//!
//! Plan: `claude-notes/plans/2026-10-13-react-components.md`.

pub mod config;
pub mod runtime;

pub use config::{
    Binding, CHROME_COMPONENT_NAMES, COMPONENTS_KEY, ComponentDecl, ComponentSource, ContentMode,
    TOC_COMPONENT_NAME, parse_components,
};
pub use runtime::{COMPONENT_HOST_ATTR, register_component_assets};
