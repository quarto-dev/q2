/*
 * mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Book project support (`ProjectKind::Book`). Port of Q1's
 * `src/project/types/book/`.
 */

pub mod citeproc;
pub mod config;
pub mod links;
pub mod merge;
pub mod project_type;
pub mod render_item;

// Native-only: drives the render-to-file tail (`crate::render_to_file` is
// gated out for wasm; WASM preview renders single documents, never books).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod single_file_render;

pub use citeproc::strip_citeproc_from_filters;
pub use links::resolve_cross_chapter_links;
pub use merge::merge_book_chapters;
pub use project_type::{BookProjectType, is_supported_format};
pub use render_item::{BookRenderItem, BookRenderItemKind, book_render_items, chapter_is_numbered};
