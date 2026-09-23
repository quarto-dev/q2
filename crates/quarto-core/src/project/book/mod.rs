/*
 * mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Book project support (`ProjectKind::Book`). Port of Q1's
 * `src/project/types/book/`.
 */

pub mod config;
pub mod project_type;
pub mod render_item;

pub use project_type::{BookProjectType, is_supported_format};
pub use render_item::{BookRenderItem, BookRenderItemKind, book_render_items, chapter_is_numbered};
