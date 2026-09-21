/*
 * ast.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! `MathAst`: the small, writer-facing tree every emitter consumes.
//!
//! The reader's CST is lossless and shaped by mitex's argument binding;
//! `MathAst` is the semantic form: attachments folded into one `Scripts`,
//! `\left…\right` resolved, environments split into rows and cells, symbols
//! resolved to Unicode, and every node carrying the byte span of the math
//! text it came from. See the "Phase 1 design" section of
//! `claude-notes/plans/2026-09-21-quarto-math-and-native-docx.md`.

use std::fmt;
use std::ops::Range;

pub use crate::spec::{EnvLayout, FracStyle, LimLoc, Pos, Variant};

/// A node with its span in the math text (byte offsets).
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    /// `None` only for nodes synthesized without any source byte (an error
    /// token's message, an empty cell added to pad a ragged row).
    pub span: Option<Range<usize>>,
}

/// What a node is. Boxed children keep the enum small.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    /// A sequence. The root is always a `Row`.
    Row(Vec<Node>),
    /// A run of math-mode characters as mitex lexed them, e.g. `i=1`.
    /// Splitting into identifiers / numbers / operators is a later pass.
    Run(String),
    /// A resolved symbol (`α`, `≤`, `→`) or a bare delimiter character.
    Sym(String),
    /// Horizontal space in em.
    Space(f32),
    Frac {
        style: FracStyle,
        num: Box<Node>,
        den: Box<Node>,
    },
    Sqrt {
        degree: Option<Box<Node>>,
        body: Box<Node>,
    },
    /// Sub- and/or superscript on a base.
    Scripts {
        base: Box<Node>,
        sub: Option<Box<Node>>,
        sup: Option<Box<Node>>,
        limits: LimLoc,
    },
    /// A big operator; its limits, if any, live in the enclosing `Scripts`.
    /// It carries no body: TeX gives a summand no scope, so it stays a
    /// sibling.
    Nary {
        text: String,
        limits: LimLoc,
    },
    /// An upright function name (`sin`, `lim`).
    Func {
        name: String,
        limits: LimLoc,
    },
    /// `\left … \right`, parts split at `\middle`. `None` is the `.` fence.
    Delimited {
        left: Option<String>,
        right: Option<String>,
        parts: Vec<Node>,
    },
    Accent {
        text: String,
        body: Box<Node>,
    },
    Bar {
        pos: Pos,
        body: Box<Node>,
    },
    GroupChr {
        text: String,
        pos: Pos,
        body: Box<Node>,
    },
    /// `\overset`/`\underset`: `annotation` above or below `body`.
    LimPos {
        pos: Pos,
        annotation: Box<Node>,
        body: Box<Node>,
    },
    /// `\xrightarrow[below]{above}`.
    XArrow {
        text: String,
        above: Option<Box<Node>>,
        below: Option<Box<Node>>,
    },
    Style {
        variant: Variant,
        body: Box<Node>,
    },
    /// Literal text (`\text{…}`), already in text mode.
    Text {
        variant: Variant,
        text: String,
    },
    Phantom {
        h: bool,
        v: bool,
        body: Box<Node>,
    },
    Cancel(Box<Node>),
    Color {
        color: String,
        body: Box<Node>,
    },
    /// An environment: rows of cells, each cell a `Row` node.
    Matrix {
        layout: EnvLayout,
        delims: Option<(String, String)>,
        rows: Vec<Vec<Node>>,
    },
    /// `\\` outside an environment.
    Break,
    /// Something the writers cannot render; `verbatim` is the source text.
    Error {
        message: String,
        verbatim: String,
    },
}

impl Node {
    pub fn new(kind: NodeKind, span: Option<Range<usize>>) -> Node {
        Node { kind, span }
    }

    /// Children in document order (cells of a matrix row by row).
    pub fn children(&self) -> Vec<&Node> {
        use NodeKind::*;
        match &self.kind {
            Row(items) => items.iter().collect(),
            Frac { num, den, .. } => vec![num, den],
            Sqrt { degree, body } => degree.iter().map(|d| &**d).chain([&**body]).collect(),
            Scripts { base, sub, sup, .. } => [Some(base), sub.as_ref(), sup.as_ref()]
                .into_iter()
                .flatten()
                .map(|b| &**b)
                .collect(),
            Delimited { parts, .. } => parts.iter().collect(),
            Accent { body, .. }
            | Bar { body, .. }
            | GroupChr { body, .. }
            | Style { body, .. }
            | Phantom { body, .. }
            | Color { body, .. } => vec![body],
            Cancel(body) => vec![body],
            LimPos {
                annotation, body, ..
            } => vec![annotation, body],
            XArrow { above, below, .. } => [below.as_ref(), above.as_ref()]
                .into_iter()
                .flatten()
                .map(|b| &**b)
                .collect(),
            Matrix { rows, .. } => rows.iter().flatten().collect(),
            Run(_)
            | Sym(_)
            | Space(_)
            | Nary { .. }
            | Func { .. }
            | Text { .. }
            | Break
            | Error { .. } => vec![],
        }
    }

    /// Depth-first walk over this node and every descendant.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Node)) {
        f(self);
        for child in self.children() {
            child.walk(f);
        }
    }

    /// Whether any node in the tree is an `Error`.
    pub fn has_errors(&self) -> bool {
        let mut found = false;
        self.walk(&mut |n| {
            if matches!(n.kind, NodeKind::Error { .. }) {
                found = true;
            }
        });
        found
    }
}

/// Compact S-expression rendering used by the snapshot tests: one node per
/// line, children indented, spans as `@start..end`.
impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        render(self, 0, f)
    }
}

fn span_str(span: &Option<Range<usize>>) -> String {
    match span {
        Some(r) => format!("@{}..{}", r.start, r.end),
        None => "@-".to_string(),
    }
}

fn render(node: &Node, depth: usize, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    use NodeKind::*;
    let pad = "  ".repeat(depth);
    let s = span_str(&node.span);
    match &node.kind {
        Row(items) => {
            writeln!(f, "{pad}row {s}")?;
            for i in items {
                render(i, depth + 1, f)?;
            }
            Ok(())
        }
        Run(t) => writeln!(f, "{pad}run {t:?} {s}"),
        Sym(t) => writeln!(f, "{pad}sym {t:?} {s}"),
        Space(em) => writeln!(f, "{pad}space {em}em {s}"),
        Frac { style, num, den } => {
            writeln!(f, "{pad}frac {style:?} {s}")?;
            render(num, depth + 1, f)?;
            render(den, depth + 1, f)
        }
        Sqrt { degree, body } => {
            writeln!(f, "{pad}sqrt {s}")?;
            if let Some(d) = degree {
                writeln!(f, "{pad}  degree:")?;
                render(d, depth + 2, f)?;
            }
            render(body, depth + 1, f)
        }
        Scripts {
            base,
            sub,
            sup,
            limits,
        } => {
            writeln!(f, "{pad}scripts {limits:?} {s}")?;
            render(base, depth + 1, f)?;
            if let Some(x) = sub {
                writeln!(f, "{pad}  sub:")?;
                render(x, depth + 2, f)?;
            }
            if let Some(x) = sup {
                writeln!(f, "{pad}  sup:")?;
                render(x, depth + 2, f)?;
            }
            Ok(())
        }
        Nary { text, limits } => writeln!(f, "{pad}nary {text:?} {limits:?} {s}"),
        Func { name, limits } => writeln!(f, "{pad}func {name:?} {limits:?} {s}"),
        Delimited { left, right, parts } => {
            writeln!(
                f,
                "{pad}delimited {:?} {:?} {s}",
                left.as_deref().unwrap_or("."),
                right.as_deref().unwrap_or(".")
            )?;
            for (i, p) in parts.iter().enumerate() {
                if i > 0 {
                    writeln!(f, "{pad}  middle:")?;
                }
                render(p, depth + 1, f)?;
            }
            Ok(())
        }
        Accent { text, body } => {
            writeln!(f, "{pad}accent {text:?} {s}")?;
            render(body, depth + 1, f)
        }
        Bar { pos, body } => {
            writeln!(f, "{pad}bar {pos:?} {s}")?;
            render(body, depth + 1, f)
        }
        GroupChr { text, pos, body } => {
            writeln!(f, "{pad}groupchr {text:?} {pos:?} {s}")?;
            render(body, depth + 1, f)
        }
        LimPos {
            pos,
            annotation,
            body,
        } => {
            writeln!(f, "{pad}limpos {pos:?} {s}")?;
            writeln!(f, "{pad}  annotation:")?;
            render(annotation, depth + 2, f)?;
            render(body, depth + 1, f)
        }
        XArrow { text, above, below } => {
            writeln!(f, "{pad}xarrow {text:?} {s}")?;
            if let Some(a) = above {
                writeln!(f, "{pad}  above:")?;
                render(a, depth + 2, f)?;
            }
            if let Some(b) = below {
                writeln!(f, "{pad}  below:")?;
                render(b, depth + 2, f)?;
            }
            Ok(())
        }
        Style { variant, body } => {
            writeln!(f, "{pad}style {variant:?} {s}")?;
            render(body, depth + 1, f)
        }
        Text { variant, text } => writeln!(f, "{pad}text {variant:?} {text:?} {s}"),
        Phantom { h, v, body } => {
            writeln!(f, "{pad}phantom h={h} v={v} {s}")?;
            render(body, depth + 1, f)
        }
        Cancel(body) => {
            writeln!(f, "{pad}cancel {s}")?;
            render(body, depth + 1, f)
        }
        Color { color, body } => {
            writeln!(f, "{pad}color {color:?} {s}")?;
            render(body, depth + 1, f)
        }
        Matrix {
            layout,
            delims,
            rows,
        } => {
            writeln!(
                f,
                "{pad}matrix {layout:?} {} {s}",
                match delims {
                    Some((l, r)) => format!("{l:?} {r:?}"),
                    None => "-".to_string(),
                }
            )?;
            for (ri, row) in rows.iter().enumerate() {
                writeln!(f, "{pad}  row {ri}:")?;
                for cell in row {
                    render(cell, depth + 2, f)?;
                }
            }
            Ok(())
        }
        Break => writeln!(f, "{pad}break {s}"),
        Error { message, verbatim } => writeln!(f, "{pad}error {message:?} {verbatim:?} {s}"),
    }
}
