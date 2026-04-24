/*
 * lib.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Document-model types for Quarto navigation.
//!
//! This crate hosts the data model and YAML resolution for Quarto's top-level
//! navigation features — navbars, page footers, and the shared
//! [`NavigationItem`] shape — so they sit alongside the existing TOC support
//! under the `navigation.*` metadata namespace.
//!
//! The crate intentionally excludes AST walkers and document-execution
//! concerns. Inputs are [`ConfigValue`](quarto_pandoc_types::config_value::ConfigValue)
//! trees (the post-merge `ast.meta`); outputs are either the resolved
//! Rust structs or `ConfigValue` trees suitable for storing back at
//! `navigation.navbar` / `navigation.footer`.
//!
//! HTML rendering of resolved structures lives in [`render_html`]. The
//! `quarto-core` crate owns the AST transforms that call into this crate.

pub mod footer;
pub mod item;
pub mod navbar;
pub mod page_nav;
pub mod render_html;
pub mod sidebar;

pub use footer::{FooterBorder, FooterRegion, PageFooter, resolve_page_footer};
pub use item::NavigationItem;
pub use navbar::{CollapseBelow, Navbar, NavbarTitle, TogglePosition, resolve_navbar};
pub use page_nav::PageNavigation;
pub use sidebar::{
    AutoSpec, Sidebar, SidebarEntry, SidebarStyle, resolve_active_state, sidebar_for_page,
};
