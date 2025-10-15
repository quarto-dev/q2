/*
 * list.rs
 * Copyright (c) 2025 Posit, PBC
 */

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ListNumberStyle {
    Default,
    Example,
    Decimal,
    LowerRoman,
    UpperRoman,
    LowerAlpha,
    UpperAlpha,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ListNumberDelim {
    Default,
    Period,
    OneParen,
    TwoParens,
}

pub type ListAttributes = (usize, ListNumberStyle, ListNumberDelim);
