#![feature(trim_prefix_suffix)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![allow(dead_code)]

/*
 * lib.rs
 * Copyright (c) 2025 Posit, PBC
 */

pub mod errors;
pub mod filter_context;
pub mod filters;
#[cfg(feature = "lua-filter")]
pub mod lua;
pub mod options;
pub mod pandoc;
pub mod readers;
pub mod template;
pub mod toc;
pub mod transforms;
pub mod traversals;
pub mod utils;
pub mod wasm_entry_points;
pub mod writers;
