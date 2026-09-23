/*
 * spec/semantics.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The writer half of a spec row: what a command *means*, independent of
//! how mitex parses its arguments. See the "Phase 1 design" section of
//! `claude-notes/plans/2026-09-21-quarto-math-and-native-docx.md` for the
//! table this enum encodes.

use serde::{Deserialize, Serialize};

/// Where a big operator's or function's limits go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LimLoc {
    /// Decide by mode: under/over in display math, sub/sup inline.
    #[default]
    Auto,
    /// Always under and over the operator.
    UndOvr,
    /// Always as sub- and superscript.
    SubSup,
}

/// Fraction drawing style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FracStyle {
    /// `\frac`, `\over`: a fraction bar.
    Bar,
    /// `\atop`: stacked without a bar.
    NoBar,
    /// `\binom`, `\choose`: stacked without a bar, inside parentheses.
    Binom,
    /// `\dfrac`: display-style fraction.
    Display,
    /// `\tfrac`: text-style fraction.
    Text,
    /// `\cfrac`: continued fraction.
    Continued,
}

/// Above or below the base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Pos {
    Top,
    Bot,
}

/// Font variant for `\mathbf`-style commands and `\text`-family text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Variant {
    /// Upright text (`\text`, `\mathrm`, `\textrm`, `\mbox`).
    Roman,
    /// The math default: italic letters (`\mathit`, `\mathnormal`).
    Italic,
    Bold,
    BoldItalic,
    DoubleStruck,
    Script,
    Fraktur,
    Sans,
    Mono,
}

/// The four `left1` operators mitex binds to the preceding item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScriptsOp {
    Sup,
    Sub,
    Limits,
    NoLimits,
}

/// How an environment lays out its rows and cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EnvLayout {
    /// `matrix` family and `array`: a grid; `delims` supply the fence.
    Matrix,
    /// `cases`: left brace, rows aligned at `&`.
    Cases,
    /// `rcases`: right brace.
    Rcases,
    /// `aligned`, `align`, `split`, `alignedat`, `equation`: rows with
    /// alignment points at `&`.
    Aligned,
    /// `gathered`, `gather`, `multline`: centered rows, no alignment point.
    Gathered,
}

/// What a command or environment means to the writers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Semantics {
    /// An ordinary symbol: Greek, relations, arrows, delimiters.
    Sym { text: String },
    /// A big operator (`\sum`, `\int`, `\bigcup`).
    Nary {
        text: String,
        #[serde(default)]
        limits: LimLoc,
    },
    /// A function name set upright (`\sin`, `\log`, `\lim`).
    Func {
        name: String,
        #[serde(default)]
        limits: LimLoc,
    },
    /// Two stacked arguments.
    Frac { style: FracStyle },
    /// `\sqrt` with an optional `[degree]`.
    Sqrt,
    /// A combining accent over one argument.
    Accent { text: String },
    /// A bar over or under one argument.
    Bar { pos: Pos },
    /// A stretchy character (brace, arrow) over or under one argument.
    GroupChr { text: String, pos: Pos },
    /// `\overset`/`\underset`/`\stackrel`: annotation then base.
    LimPos { pos: Pos },
    /// `\xrightarrow[below]{above}`: a stretchy arrow with annotations.
    XArrow { text: String },
    /// A font variant applied to math content.
    Style { variant: Variant },
    /// Literal text set in a variant (`\text`, `\textbf`, `\operatorname`).
    Text { variant: Variant },
    /// Horizontal space, in em; negative for `\!`.
    Space { em: f32 },
    /// Invisible box keeping width and/or height.
    Phantom { h: bool, v: bool },
    /// Diagonal strike-through (`\cancel`).
    Cancel,
    /// Negate the next symbol with U+0338.
    Not,
    /// `\color{c}` / `\textcolor{c}{x}`: first argument is the color.
    Color,
    /// `^`, `_`, `\limits`, `\nolimits`.
    Scripts { which: ScriptsOp },
    /// An environment.
    Env {
        layout: EnvLayout,
        /// Opening and closing fence, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delims: Option<(String, String)>,
        /// `array`: the column spec is the first argument.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        cols_from_arg: bool,
    },
    /// Parsed and dropped (`\displaystyle`, `\nonumber`, …).
    Ignore,
    /// Known to mitex, not renderable by any writer; the writer reports it
    /// and falls back to verbatim TeX.
    Unsupported { why: String },
}

impl Semantics {
    /// The Unicode text a symbol-like row contributes, if any.
    pub fn text(&self) -> Option<&str> {
        match self {
            Semantics::Sym { text }
            | Semantics::Nary { text, .. }
            | Semantics::Accent { text }
            | Semantics::GroupChr { text, .. }
            | Semantics::XArrow { text } => Some(text),
            _ => None,
        }
    }

    /// Whether this row renders to nothing but a fallback.
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Semantics::Unsupported { .. })
    }
}
