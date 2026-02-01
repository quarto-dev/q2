/*
 * shortcode.rs
 * Copyright (c) 2025 Posit, PBC
 */

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shortcode {
    pub is_escaped: bool,
    pub name: String,
    pub positional_args: Vec<ShortcodeArg>,
    pub keyword_args: HashMap<String, ShortcodeArg>,
    pub source_info: quarto_source_map::SourceInfo,
}
