/*
 * caption.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::block::Blocks;
use crate::pandoc::inline::Inlines;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Caption {
    pub short: Option<Inlines>,
    pub long: Option<Blocks>,
}
