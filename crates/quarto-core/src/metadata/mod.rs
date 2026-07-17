/*
 * metadata/mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Typed views over document metadata.
 */

//! Typed views over document metadata.
//!
//! This module hosts the typed models that normalize raw `ConfigValue`
//! metadata into shapes the render pipeline (templates, document
//! profile, preview) can consume without re-parsing config maps.
//!
//! Introduced by the title-block parity epic (bd-gx9cic8z); see
//! `claude-notes/plans/2026-07-15-html-title-block-parity.md`.

pub mod authors;
