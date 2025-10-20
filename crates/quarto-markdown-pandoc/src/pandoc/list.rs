/*
 * list.rs
 * Copyright (c) 2025 Posit, PBC
 */

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ListNumberStyle {
    Default,
    Example,
    Decimal,
    LowerRoman,
    UpperRoman,
    LowerAlpha,
    UpperAlpha,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ListNumberDelim {
    Default,
    Period,
    OneParen,
    TwoParens,
}

pub type ListAttributes = (usize, ListNumberStyle, ListNumberDelim);
