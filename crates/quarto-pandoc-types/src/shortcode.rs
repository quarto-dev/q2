/*
 * shortcode.rs
 * Copyright (c) 2025 Posit, PBC
 */

use hashlink::LinkedHashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ShortcodeArg {
    String(String),
    Number(f64),
    Boolean(bool),
    Shortcode(Shortcode),
    KeyValue(HashMap<String, ShortcodeArg>),
}

/// A Quarto shortcode (e.g. `{{< video https://example.com >}}`).
///
/// `keyword_args` uses `LinkedHashMap` so insertion order is preserved on
/// iteration. The qmd writer relies on this to produce a stable, source-order
/// round-trip — see `crates/pampa/src/writers/qmd.rs::write_shortcode`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shortcode {
    pub is_escaped: bool,
    pub name: String,
    pub positional_args: Vec<ShortcodeArg>,
    pub keyword_args: LinkedHashMap<String, ShortcodeArg>,
    pub source_info: quarto_source_map::SourceInfo,
}
