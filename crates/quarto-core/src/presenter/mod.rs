/*
 * presenter/mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-1: target-neutral presenter (cell outputs to Quarto cell markdown).
 */

//! The presenter turns kernel-shaped outputs (nbformat mime bundles, streams,
//! errors) into Quarto cell markdown.
//!
//! This module is a sibling of `engine` and is **not** gated on `wasm32`: nothing in
//! it may depend on `jupyter_protocol`, `tokio` or any other native-only item, so the
//! live Jupyter path, the stored-notebook replay and (later) record presentation can
//! all call the same functions on every target.

pub mod cell;
pub mod figures;
pub mod inline;
pub mod output;
#[cfg(test)]
pub(crate) mod recording_runtime;
pub mod visibility;
