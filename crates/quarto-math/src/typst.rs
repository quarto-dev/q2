/*
 * typst.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! `MathAst` → plain Typst math.
//!
//! The output is the *inside* of a Typst math block (`$ … $`), written
//! against Typst's built-in math functions only, so a Quarto Typst document
//! imports nothing (mitex's own converter would make every document import
//! the mitex package). Every syntax choice below was compiled with Typst
//! before it was adopted; the corpus test compiles the whole fixture set.
//!
//! Tokens are separated by spaces because Typst math reads adjacent letters
//! as one identifier: a coarse mitex run (`4ac`) is split by
//! [`crate::split`] and emitted as `4 a c`, which is what TeX meant.

use std::fmt::Write;

use crate::ast::{EnvLayout, FracStyle, LimLoc, Node, NodeKind, Pos, Variant};
use crate::normalize::Normalized;
use crate::split::{PieceKind, split_run};

/// Typst math for a normalized expression (no `$` delimiters).
pub fn to_typst(n: &Normalized) -> String {
    let mut w = Writer::default();
    w.node(&n.root)
}

#[derive(Default)]
struct Writer {}

/// Characters that carry syntax in Typst math and must be escaped when they
/// are meant literally.
fn escape_math_char(c: char, out: &mut String) {
    match c {
        '/' | '_' | '^' | '{' | '}' | '&' | '#' | '$' | '\\' | '"' | '@' | '*' => {
            out.push('\\');
            out.push(c);
        }
        _ => out.push(c),
    }
}

fn escape_math(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for c in text.chars() {
        escape_math_char(c, &mut out);
    }
    out
}

/// A Typst string literal.
fn string_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Typst's name for a big operator, or the character itself. Names were
/// checked against Typst 0.15; `∐`, `⨁`, `⨂` and `⨀` have none there.
fn nary_name(text: &str) -> String {
    match text {
        "∑" => "sum",
        "∏" => "product",
        "∫" => "integral",
        "∬" => "integral.double",
        "∭" => "integral.triple",
        "⨌" => "integral.quad",
        "∮" => "integral.cont",
        "∯" => "integral.surf",
        "∰" => "integral.vol",
        "⋃" => "union.big",
        "⋂" => "inter.big",
        "⨆" => "union.sq.big",
        "⋁" => "or.big",
        "⋀" => "and.big",
        "⨄" => "union.plus.big",
        "⨉" => "times.big",
        other => return escape_math(other),
    }
    .to_string()
}

/// Function names Typst's math mode already defines as operators.
const TYPST_OPS: &[&str] = &[
    "arccos", "arcsin", "arctan", "arg", "cos", "cosh", "cot", "coth", "csc", "csch", "ctg", "deg",
    "det", "dim", "exp", "gcd", "lcm", "hom", "id", "im", "inf", "ker", "lg", "lim", "liminf",
    "limsup", "ln", "log", "max", "min", "mod", "Pr", "sec", "sech", "sin", "sinc", "sinh", "sup",
    "tan", "tanh", "tg", "tr",
];

fn accent_name(text: &str) -> Option<&'static str> {
    Some(match text {
        "\u{302}" => "hat",
        "\u{303}" => "tilde",
        "\u{304}" => "macron",
        "\u{306}" => "breve",
        "\u{307}" => "dot",
        "\u{308}" => "dot.double",
        "\u{20db}" => "dot.triple",
        "\u{20dc}" => "dot.quad",
        "\u{301}" => "acute",
        "\u{30b}" => "acute.double",
        "\u{300}" => "grave",
        "\u{30c}" => "caron",
        "\u{30a}" => "circle",
        "\u{20d7}" => "arrow",
        "\u{20d6}" => "arrow.l",
        "\u{20e1}" => "arrow.l.r",
        "\u{20d1}" => "harpoon",
        "\u{20d0}" => "harpoon.lt",
        _ => return None,
    })
}

fn group_chr_name(text: &str, pos: Pos) -> &'static str {
    match (text, pos) {
        ("⏞", _) => "overbrace",
        ("⏟", _) => "underbrace",
        ("⎴", _) => "overbracket",
        ("⎵", _) => "underbracket",
        (_, Pos::Top) => "overbrace",
        (_, Pos::Bot) => "underbrace",
    }
}

fn color_hex(name: &str) -> String {
    let name = name.trim();
    match name.to_ascii_lowercase().as_str() {
        "red" => "FF0000",
        "green" => "00FF00",
        "blue" => "0000FF",
        "cyan" => "00FFFF",
        "magenta" => "FF00FF",
        "yellow" => "FFFF00",
        "black" => "000000",
        "white" => "FFFFFF",
        "gray" | "grey" => "808080",
        "lightgray" | "lightgrey" => "C0C0C0",
        "darkgray" | "darkgrey" => "404040",
        "brown" => "A52A2A",
        "orange" => "FFA500",
        "pink" => "FFB6C1",
        "purple" => "800080",
        "teal" => "008080",
        "olive" => "808000",
        "violet" => "EE82EE",
        _ => {
            let bare = name.trim_start_matches('#');
            if bare.len() == 6 && bare.chars().all(|c| c.is_ascii_hexdigit()) {
                return bare.to_ascii_uppercase();
            }
            "000000"
        }
    }
    .to_string()
}

fn format_em(em: f32) -> String {
    let s = format!("{em:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

fn space(em: f32) -> String {
    if em <= 0.0 {
        return format!("#h(-{}em)", format_em(-em));
    }
    let close = |target: f32| (em - target).abs() < 1e-3;
    if close(0.1667) {
        "thin".to_string()
    } else if close(0.2222) {
        "med".to_string()
    } else if close(0.2778) {
        "thick".to_string()
    } else if close(1.0) {
        "quad".to_string()
    } else if close(2.0) {
        "wide".to_string()
    } else {
        format!("#h({}em)", format_em(em))
    }
}

/// `mat(delim: …)` value for an opening fence.
fn mat_delim(delims: Option<&(String, String)>) -> String {
    match delims {
        None => "#none".to_string(),
        Some((l, _)) => string_literal(l),
    }
}

impl Writer {
    fn items(&mut self, items: &[Node]) -> String {
        let parts: Vec<String> = items
            .iter()
            .map(|n| self.node(n))
            .filter(|s| !s.is_empty())
            .collect();
        parts.join(" ")
    }

    /// A node as an attachment base: compound content gets parentheses, and
    /// an empty base (`^2` alone) becomes an empty string so the attachment
    /// has something to hang on.
    fn base(&mut self, node: &Node) -> String {
        let s = self.node(node);
        if s.is_empty() {
            return "\"\"".to_string();
        }
        let compound = match &node.kind {
            NodeKind::Row(items) => items.len() > 1,
            NodeKind::Frac { .. } => true,
            _ => false,
        };
        if compound { format!("({s})") } else { s }
    }

    fn scripts_suffix(&mut self, sub: Option<&Node>, sup: Option<&Node>) -> String {
        let mut s = String::new();
        if let Some(sub) = sub {
            let _ = write!(s, "_({})", self.node(sub));
        }
        if let Some(sup) = sup {
            let _ = write!(s, "^({})", self.node(sup));
        }
        s
    }

    fn func_name(&mut self, name: &str, limits: LimLoc) -> String {
        if TYPST_OPS.contains(&name) {
            return name.to_string();
        }
        if limits == LimLoc::UndOvr {
            format!("op({}, limits: #true)", string_literal(name))
        } else {
            format!("op({})", string_literal(name))
        }
    }

    fn nary(&mut self, text: &str, limits: LimLoc) -> String {
        let name = nary_name(text);
        match limits {
            LimLoc::UndOvr => format!("limits({name})"),
            LimLoc::SubSup => format!("scripts({name})"),
            LimLoc::Auto => name,
        }
    }

    fn run(&mut self, text: &str) -> String {
        let pieces = split_run(text, None);
        let mut out = String::new();
        for piece in pieces {
            if !out.is_empty() && piece.text != "'" {
                out.push(' ');
            }
            match piece.kind {
                PieceKind::Identifier | PieceKind::Number => out.push_str(&piece.text),
                PieceKind::Operator => out.push_str(&escape_math(&piece.text)),
            }
        }
        out
    }

    fn node(&mut self, node: &Node) -> String {
        use NodeKind::*;
        match &node.kind {
            Row(items) => self.items(items),
            Run(t) => self.run(t),
            Sym(t) => escape_math(t),
            Space(em) => space(*em),
            Frac { style, num, den } => {
                let n = self.node(num);
                let d = self.node(den);
                match style {
                    FracStyle::Bar | FracStyle::Continued => format!("({n})/({d})"),
                    FracStyle::Display => format!("display(({n})/({d}))"),
                    FracStyle::Text => format!("inline(({n})/({d}))"),
                    FracStyle::NoBar => format!("mat(delim: #none, {n}; {d})"),
                    FracStyle::Binom => format!("binom({n}, {d})"),
                }
            }
            Sqrt { degree, body } => {
                let b = self.node(body);
                match degree {
                    Some(d) => format!("root({}, {b})", self.node(d)),
                    None => format!("sqrt({b})"),
                }
            }
            Scripts {
                base,
                sub,
                sup,
                limits,
            } => {
                let suffix = self.scripts_suffix(sub.as_deref(), sup.as_deref());
                let inner = unwrap_row(base);
                let head = match &inner.kind {
                    Nary { text, .. } => self.nary(text, *limits),
                    Func { name, .. } => self.func_name(name, *limits),
                    GroupChr { text, pos, body }
                        if (sub.is_some() && sup.is_none() && *pos == Pos::Bot)
                            || (sup.is_some() && sub.is_none() && *pos == Pos::Top) =>
                    {
                        // `\underbrace{x}_{n}`: the annotation is Typst's
                        // second argument, not a script.
                        let ann = sub.as_deref().or(sup.as_deref()).unwrap();
                        return format!(
                            "{}({}, {})",
                            group_chr_name(text, *pos),
                            self.node(body),
                            self.node(ann)
                        );
                    }
                    _ => self.base(base),
                };
                format!("{head}{suffix}")
            }
            Nary { text, limits } => self.nary(text, *limits),
            Func { name, limits } => self.func_name(name, *limits),
            Delimited { left, right, parts } => {
                // Fences are escaped characters: Typst's parser wants
                // balanced brackets even inside `lr`, and `\(` is a delimiter
                // `lr` still sizes, so one-sided fences stay valid.
                let body: Vec<String> = parts.iter().map(|p| self.node(p)).collect();
                let mut s = String::from("lr(");
                if let Some(l) = left {
                    s.push_str(&fence(l));
                    s.push(' ');
                }
                s.push_str(&body.join(" mid(|) "));
                if let Some(r) = right {
                    s.push(' ');
                    s.push_str(&fence(r));
                }
                s.push(')');
                s
            }
            Accent { text, body } => {
                let b = self.node(body);
                match accent_name(text) {
                    Some(name) => format!("{name}({b})"),
                    None => format!("accent({b}, {})", string_literal(text)),
                }
            }
            Bar { pos, body } => {
                let b = self.node(body);
                match pos {
                    Pos::Top => format!("overline({b})"),
                    Pos::Bot => format!("underline({b})"),
                }
            }
            GroupChr { text, pos, body } => {
                format!("{}({})", group_chr_name(text, *pos), self.node(body))
            }
            LimPos {
                pos,
                annotation,
                body,
            } => {
                let b = self.node(body);
                let a = self.node(annotation);
                match pos {
                    Pos::Top => format!("limits({b})^({a})"),
                    Pos::Bot => format!("limits({b})_({a})"),
                }
            }
            XArrow { text, above, below } => {
                let mut s = format!("stretch({})", escape_math(text));
                if let Some(a) = above {
                    let _ = write!(s, "^({})", self.node(a));
                }
                if let Some(b) = below {
                    let _ = write!(s, "_({})", self.node(b));
                }
                s
            }
            Style { variant, body } => {
                let b = self.node(body);
                match variant {
                    Variant::Roman => format!("upright({b})"),
                    Variant::Italic => format!("italic({b})"),
                    Variant::Bold => format!("bold({b})"),
                    Variant::BoldItalic => format!("bold(italic({b}))"),
                    Variant::DoubleStruck => format!("bb({b})"),
                    Variant::Script => format!("cal({b})"),
                    Variant::Fraktur => format!("frak({b})"),
                    Variant::Sans => format!("sans({b})"),
                    Variant::Mono => format!("mono({b})"),
                }
            }
            Text { variant, text } => {
                let s = string_literal(text);
                match variant {
                    Variant::Bold => format!("bold({s})"),
                    Variant::Italic => format!("italic({s})"),
                    Variant::BoldItalic => format!("bold(italic({s}))"),
                    Variant::Sans => format!("sans({s})"),
                    Variant::Mono => format!("mono({s})"),
                    _ => s,
                }
            }
            Phantom { body, .. } => format!("std.hide({})", self.node(body)),
            Cancel(body) => format!("cancel({})", self.node(body)),
            Color { color, body } => format!(
                "text(fill: #rgb({}), {})",
                string_literal(&color_hex(color)),
                self.node(body)
            ),
            Matrix {
                layout,
                delims,
                rows,
            } => {
                let cells: Vec<Vec<String>> = rows
                    .iter()
                    .map(|r| r.iter().map(|c| self.node(c)).collect())
                    .collect();
                match layout {
                    EnvLayout::Matrix => {
                        let body: Vec<String> = cells.iter().map(|r| r.join(", ")).collect();
                        format!(
                            "mat(delim: {}, {})",
                            mat_delim(delims.as_ref()),
                            body.join("; ")
                        )
                    }
                    EnvLayout::Cases | EnvLayout::Rcases => {
                        let body: Vec<String> = cells.iter().map(|r| r.join(" & ")).collect();
                        if *layout == EnvLayout::Rcases {
                            format!("cases(reverse: #true, {})", body.join(", "))
                        } else {
                            format!("cases({})", body.join(", "))
                        }
                    }
                    EnvLayout::Aligned => {
                        let body: Vec<String> = cells.iter().map(|r| r.join(" & ")).collect();
                        body.join(" \\ ")
                    }
                    EnvLayout::Gathered => {
                        let body: Vec<String> = cells.iter().map(|r| r.join(" ")).collect();
                        body.join(" \\ ")
                    }
                }
            }
            Break => "\\".to_string(),
            Error { verbatim, .. } => string_literal(verbatim),
        }
    }
}

/// A fence character for `lr`: brackets escaped, everything else literal.
fn fence(text: &str) -> String {
    match text {
        "(" | ")" | "[" | "]" | "{" | "}" => format!("\\{text}"),
        other => escape_math(other),
    }
}

fn unwrap_row(node: &Node) -> &Node {
    match &node.kind {
        NodeKind::Row(items) if items.len() == 1 => unwrap_row(&items[0]),
        _ => node,
    }
}
