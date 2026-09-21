/*
 * mathml.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! `MathAst` → MathML Core (bd-9z83tcv0).
//!
//! The output is one complete `<math>` element, ready to drop into HTML,
//! written against [MathML Core] only: the subset every current browser
//! renders natively. Two consequences shape the writer:
//!
//! - **Token classification is ours.** A coarse mitex run (`4ac`, `i=1`)
//!   is split by [`crate::split`] into `mi` / `mn` / `mo`, one letter per
//!   `mi` (TeX semantics, where `ab` is two variables). A single-letter
//!   `mi` is italic by default in MathML, as in TeX.
//! - **No `mathvariant` but `normal`.** Core dropped the other values, so
//!   `\mathbf`, `\mathbb`, `\mathcal`, … map each character into the
//!   Mathematical Alphanumeric Symbols block (`𝐱`, `ℝ`, `𝒫`), holes
//!   included; `\mathrm` alone uses `mathvariant="normal"`.
//!
//! The source TeX rides along in `<semantics>` as an
//! `<annotation encoding="application/x-tex">`, for copy/paste and
//! assistive tools. One knowingly non-Core element remains: `\cancel`
//! emits `<menclose notation="updiagonalstrike">`, which Firefox draws and
//! Chrome ignores (the content still shows); see the plan's decision 3.
//!
//! [MathML Core]: https://www.w3.org/TR/mathml-core/

use std::fmt::Write;

use crate::ast::{EnvLayout, FracStyle, LimLoc, Node, NodeKind, Pos, Variant};
use crate::normalize::{Mode, Normalized};
use crate::split::{PieceKind, split_run};

/// Big operators whose limits TeX sets beside the operator even in display
/// math (the integral family); everything else gets them under and over.
const SIDE_LIMIT_OPERATORS: &str = "∫∬∭∮∯∰⨌";

/// Delimiter characters that appear bare in the source (`(x)`, `[a]`,
/// `\{`, `|`). TeX does not stretch them, so neither do we; `\left…\right`
/// fences are emitted separately and stretch.
const BARE_DELIMITERS: &str = "()[]{}|‖⟨⟩⌈⌉⌊⌋/\\";

/// A complete `<math>` element for a normalized expression.
pub fn to_mathml(n: &Normalized) -> String {
    let mut w = Writer {
        out: String::new(),
        mode: n.mode,
    };
    match n.mode {
        Mode::Display => w
            .out
            .push_str(r#"<math xmlns="http://www.w3.org/1998/Math/MathML" display="block">"#),
        Mode::Inline => w
            .out
            .push_str(r#"<math xmlns="http://www.w3.org/1998/Math/MathML">"#),
    }
    w.out.push_str("<semantics>");
    w.root(&n.root);
    let _ = write!(
        w.out,
        r#"<annotation encoding="application/x-tex">{}</annotation></semantics></math>"#,
        escape_text(&n.text)
    );
    w.out
}

struct Writer {
    out: String,
    mode: Mode,
}

/// Text content escaping (`&`, `<`, `>`).
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Attribute value escaping (adds `"`).
fn escape_attr(text: &str) -> String {
    escape_text(text).replace('"', "&quot;")
}

/// The spacing character MathML uses for a TeX accent, given the combining
/// character the spec resolves the command to. Combining marks render
/// poorly as `mover` content; the spacing forms are what the operator
/// dictionary knows as accents.
fn accent_char(combining: &str) -> &str {
    match combining {
        "\u{302}" => "\u{2c6}",   // hat → ˆ
        "\u{303}" => "\u{2dc}",   // tilde → ˜
        "\u{304}" => "\u{af}",    // bar → ¯
        "\u{306}" => "\u{2d8}",   // breve → ˘
        "\u{307}" => "\u{2d9}",   // dot → ˙
        "\u{308}" => "\u{a8}",    // ddot → ¨
        "\u{20db}" => "\u{20db}", // dddot has no spacing form
        "\u{20dc}" => "\u{20dc}", // ddddot has no spacing form
        "\u{301}" => "\u{b4}",    // acute → ´
        "\u{30b}" => "\u{2dd}",   // double acute → ˝
        "\u{300}" => "`",         // grave
        "\u{30c}" => "\u{2c7}",   // check → ˇ
        "\u{30a}" => "\u{2da}",   // ring → ˚
        "\u{20d7}" => "\u{2192}", // vec → →
        "\u{20d6}" => "\u{2190}", // overleftarrow accent → ←
        "\u{20e1}" => "\u{2194}", // overleftrightarrow accent → ↔
        "\u{20d1}" => "\u{21c0}", // harpoon → ⇀
        "\u{20d0}" => "\u{21bc}", // left harpoon → ↼
        other => other,
    }
}

/// `\textcolor` names to CSS colors. Named CSS colors are passed through
/// (CSS knows every name TeX's `color` package knows); `#rrggbb` and bare
/// hex triplets are normalized to `#rrggbb`.
fn css_color(name: &str) -> String {
    let name = name.trim();
    let bare = name.trim_start_matches('#');
    if bare.len() == 6 && bare.chars().all(|c| c.is_ascii_hexdigit()) {
        return format!("#{}", bare.to_ascii_lowercase());
    }
    name.to_ascii_lowercase()
}

fn format_em(em: f32) -> String {
    let s = format!("{em:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

/// Map one character into the Mathematical Alphanumeric Symbols block for
/// a font variant. Characters the variant has no glyph for (a digit in
/// script, punctuation) are returned unchanged.
pub fn styled_char(c: char, variant: Variant) -> char {
    use Variant::*;
    let holes: &[(char, char)] = match variant {
        Script => &[
            ('B', 'ℬ'),
            ('E', 'ℰ'),
            ('F', 'ℱ'),
            ('H', 'ℋ'),
            ('I', 'ℐ'),
            ('L', 'ℒ'),
            ('M', 'ℳ'),
            ('R', 'ℛ'),
            ('e', 'ℯ'),
            ('g', 'ℊ'),
            ('o', 'ℴ'),
        ],
        Fraktur => &[('C', 'ℭ'), ('H', 'ℌ'), ('I', 'ℑ'), ('R', 'ℜ'), ('Z', 'ℨ')],
        DoubleStruck => &[
            ('C', 'ℂ'),
            ('H', 'ℍ'),
            ('N', 'ℕ'),
            ('P', 'ℙ'),
            ('Q', 'ℚ'),
            ('R', 'ℝ'),
            ('Z', 'ℤ'),
        ],
        Italic => &[('h', 'ℎ')],
        _ => &[],
    };
    if let Some((_, mapped)) = holes.iter().find(|(from, _)| *from == c) {
        return *mapped;
    }
    // (upper A, lower a, digit 0, upper Α, lower α) block starts; 0 = none.
    let (upper, lower, digit, greek_upper, greek_lower) = match variant {
        Roman => return c,
        Bold => (0x1D400, 0x1D41A, 0x1D7CE, 0x1D6A8, 0x1D6C2),
        Italic => (0x1D434, 0x1D44E, 0, 0x1D6E2, 0x1D6FC),
        BoldItalic => (0x1D468, 0x1D482, 0, 0x1D71C, 0x1D736),
        Script => (0x1D49C, 0x1D4B6, 0, 0, 0),
        Fraktur => (0x1D504, 0x1D51E, 0, 0, 0),
        DoubleStruck => (0x1D538, 0x1D552, 0x1D7D8, 0, 0),
        Sans => (0x1D5A0, 0x1D5BA, 0x1D7E2, 0, 0),
        Mono => (0x1D670, 0x1D68A, 0x1D7F6, 0, 0),
    };
    let mapped = match c {
        'A'..='Z' if upper != 0 => upper + (c as u32 - 'A' as u32),
        'a'..='z' if lower != 0 => lower + (c as u32 - 'a' as u32),
        '0'..='9' if digit != 0 => digit + (c as u32 - '0' as u32),
        // Α..Ω (U+0391..U+03A9; U+03A2 is unassigned and the block puts ϴ
        // there, so the offset is direct).
        'Α'..='Ω' if greek_upper != 0 && c != '\u{3a2}' => greek_upper + (c as u32 - 'Α' as u32),
        'α'..='ω' if greek_lower != 0 => greek_lower + (c as u32 - 'α' as u32),
        _ => return c,
    };
    char::from_u32(mapped).unwrap_or(c)
}

fn styled(text: &str, variant: Option<Variant>) -> String {
    match variant {
        None | Some(Variant::Roman) => text.to_string(),
        Some(v) => text.chars().map(|c| styled_char(c, v)).collect(),
    }
}

/// Uppercase Greek is upright in TeX; a lone `mi` would italicize it.
fn is_upright_greek(text: &str) -> bool {
    let mut chars = text.chars();
    matches!((chars.next(), chars.next()), (Some('Α'..='Ω'), None))
}

impl Writer {
    /// The root: an `mrow`, or an `mtable` of lines when `\\` appears at
    /// the top level (TeX would need an environment; we do what the author
    /// meant).
    fn root(&mut self, root: &Node) {
        let items = match &root.kind {
            NodeKind::Row(items) => items.as_slice(),
            _ => std::slice::from_ref(root),
        };
        if items.iter().any(|n| matches!(n.kind, NodeKind::Break)) {
            self.out.push_str("<mtable>");
            for line in split_at_breaks(items) {
                self.out.push_str("<mtr><mtd>");
                self.mrow(line, None);
                self.out.push_str("</mtd></mtr>");
            }
            self.out.push_str("</mtable>");
        } else {
            self.mrow(items, None);
        }
    }

    /// `<mrow>` around a sequence.
    fn mrow(&mut self, items: &[Node], variant: Option<Variant>) {
        self.out.push_str("<mrow>");
        for item in items {
            self.node(item, variant);
        }
        self.out.push_str("</mrow>");
    }

    /// One node in a position that needs exactly one element (a script
    /// base, a fraction part): a single-item row unwraps, and anything
    /// that would emit several elements (a sequence, a multi-token run, a
    /// function name with its application operator) gets an `mrow`.
    fn arg(&mut self, node: &Node, variant: Option<Variant>) {
        match &node.kind {
            NodeKind::Row(items) if items.len() == 1 => self.arg(&items[0], variant),
            NodeKind::Row(items) => self.mrow(items, variant),
            _ if !emits_one_element(node) => {
                self.out.push_str("<mrow>");
                self.node(node, variant);
                self.out.push_str("</mrow>");
            }
            _ => self.node(node, variant),
        }
    }

    fn run(&mut self, text: &str, variant: Option<Variant>) {
        for piece in split_run(text, None) {
            match piece.kind {
                PieceKind::Identifier => self.mi(&piece.text, variant),
                PieceKind::Number => {
                    let _ = write!(
                        self.out,
                        "<mn>{}</mn>",
                        escape_text(&styled(&piece.text, variant))
                    );
                }
                PieceKind::Operator => {
                    // TeX's `-` is a minus sign in math; a hyphen renders
                    // short. `'` outside an attachment is a prime.
                    let text = match piece.text.as_str() {
                        "-" => "\u{2212}",
                        "'" => "′",
                        other => other,
                    };
                    self.mo(text, variant, false);
                }
            }
        }
    }

    fn mi(&mut self, text: &str, variant: Option<Variant>) {
        let upright = matches!(variant, Some(Variant::Roman)) || is_upright_greek(text);
        if upright && text.chars().count() == 1 {
            let _ = write!(
                self.out,
                r#"<mi mathvariant="normal">{}</mi>"#,
                escape_text(text)
            );
        } else {
            let _ = write!(self.out, "<mi>{}</mi>", escape_text(&styled(text, variant)));
        }
    }

    /// An operator; `fence` marks a `\left…\right` delimiter, which
    /// stretches. Bare delimiter characters explicitly do not.
    fn mo(&mut self, text: &str, variant: Option<Variant>, fence: bool) {
        let bare = !fence
            && text.chars().count() == 1
            && text
                .chars()
                .next()
                .is_some_and(|c| BARE_DELIMITERS.contains(c));
        let attrs = if fence {
            r#" stretchy="true""#
        } else if bare {
            r#" stretchy="false""#
        } else {
            ""
        };
        let _ = write!(
            self.out,
            "<mo{attrs}>{}</mo>",
            escape_text(&styled(text, variant))
        );
    }

    fn sym(&mut self, text: &str, variant: Option<Variant>) {
        let alphabetic = text.chars().all(|c| c.is_alphabetic());
        if alphabetic {
            self.mi(text, variant);
        } else {
            self.mo(text, variant, false);
        }
    }

    fn under_over(&self, limits: LimLoc, operator: &str) -> bool {
        match limits {
            LimLoc::UndOvr => true,
            LimLoc::SubSup => false,
            LimLoc::Auto => {
                let side = operator
                    .chars()
                    .next()
                    .is_some_and(|c| SIDE_LIMIT_OPERATORS.contains(c));
                self.mode == Mode::Display && !side
            }
        }
    }

    /// Attach `sub`/`sup` to an already-emitted-by-closure base as scripts
    /// (`msub`…) or limits (`munder`…).
    fn attach(
        &mut self,
        base: impl FnOnce(&mut Self),
        sub: Option<&Node>,
        sup: Option<&Node>,
        under_over: bool,
        variant: Option<Variant>,
    ) {
        let tag = match (sub.is_some(), sup.is_some(), under_over) {
            (false, false, _) => {
                base(self);
                return;
            }
            (true, true, false) => "msubsup",
            (true, false, false) => "msub",
            (false, true, false) => "msup",
            (true, true, true) => "munderover",
            (true, false, true) => "munder",
            (false, true, true) => "mover",
        };
        let _ = write!(self.out, "<{tag}>");
        base(self);
        if let Some(sub) = sub {
            self.arg(sub, variant);
        }
        if let Some(sup) = sup {
            self.arg(sup, variant);
        }
        let _ = write!(self.out, "</{tag}>");
    }

    fn nary(
        &mut self,
        text: &str,
        limits: LimLoc,
        sub: Option<&Node>,
        sup: Option<&Node>,
        variant: Option<Variant>,
    ) {
        let under_over = self.under_over(limits, text);
        // `movablelimits` is what lets the operator dictionary move limits
        // to the side inline; when the author forced `\limits`, pin them.
        let attrs = if limits == LimLoc::UndOvr {
            r#" movablelimits="false""#
        } else {
            ""
        };
        let text = text.to_string();
        self.attach(
            |w| {
                let _ = write!(w.out, "<mo{attrs}>{}</mo>", escape_text(&text));
            },
            sub,
            sup,
            under_over,
            variant,
        );
    }

    fn func(
        &mut self,
        name: &str,
        limits: LimLoc,
        sub: Option<&Node>,
        sup: Option<&Node>,
        variant: Option<Variant>,
    ) {
        let under_over = self.under_over(limits, name);
        let name = name.to_string();
        self.attach(
            |w| {
                let _ = write!(w.out, "<mi>{}</mi>", escape_text(&name));
            },
            sub,
            sup,
            under_over,
            variant,
        );
        // U+2061 FUNCTION APPLICATION: invisible, but it tells layout and
        // assistive technology that `sin x` is an application.
        self.out.push_str("<mo>\u{2061}</mo>");
    }

    fn group_chr(&mut self, text: &str, pos: Pos, body: &Node, variant: Option<Variant>) {
        let (tag, attr) = match pos {
            Pos::Top => ("mover", "accent"),
            Pos::Bot => ("munder", "accentunder"),
        };
        let _ = write!(self.out, r#"<{tag} {attr}="true">"#);
        self.arg(body, variant);
        let _ = write!(
            self.out,
            r#"<mo stretchy="true">{}</mo></{tag}>"#,
            escape_text(text)
        );
    }

    fn scripts(
        &mut self,
        base: &Node,
        sub: Option<&Node>,
        sup: Option<&Node>,
        limits: LimLoc,
        variant: Option<Variant>,
    ) {
        match &unwrap_row(base).kind {
            NodeKind::Nary { text, .. } => return self.nary(text, limits, sub, sup, variant),
            NodeKind::Func { name, .. } => return self.func(name, limits, sub, sup, variant),
            // `\underbrace{x}_{n}`: the script is the brace's label.
            NodeKind::GroupChr {
                text,
                pos: Pos::Bot,
                body,
            } if sub.is_some() && sup.is_none() => {
                self.out.push_str("<munder>");
                self.group_chr(text, Pos::Bot, body, variant);
                self.arg(sub.unwrap(), variant);
                self.out.push_str("</munder>");
                return;
            }
            NodeKind::GroupChr {
                text,
                pos: Pos::Top,
                body,
            } if sup.is_some() && sub.is_none() => {
                self.out.push_str("<mover>");
                self.group_chr(text, Pos::Top, body, variant);
                self.arg(sup.unwrap(), variant);
                self.out.push_str("</mover>");
                return;
            }
            _ => {}
        }
        self.attach(|w| w.arg(base, variant), sub, sup, false, variant);
    }

    fn matrix(
        &mut self,
        layout: EnvLayout,
        delims: Option<&(String, String)>,
        rows: &[Vec<Node>],
        variant: Option<Variant>,
    ) {
        let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let (open, close, align): (Option<&str>, Option<&str>, Option<String>) = match layout {
            EnvLayout::Matrix => (
                delims.map(|(l, _)| l.as_str()),
                delims.map(|(_, r)| r.as_str()),
                None,
            ),
            EnvLayout::Cases => (Some("{"), None, Some("left".to_string())),
            EnvLayout::Rcases => (None, Some("}"), Some("left".to_string())),
            EnvLayout::Aligned => (
                None,
                None,
                Some(
                    (0..cols)
                        .map(|i| if i % 2 == 0 { "right" } else { "left" })
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
            ),
            EnvLayout::Gathered => (None, None, None),
        };
        let fenced = open.is_some() || close.is_some();
        if fenced {
            self.out.push_str("<mrow>");
            if let Some(l) = open {
                self.mo(l, None, true);
            }
        }
        match align {
            Some(a) => {
                let _ = write!(self.out, r#"<mtable columnalign="{a}">"#);
            }
            None => self.out.push_str("<mtable>"),
        }
        if rows.is_empty() {
            self.out.push_str("<mtr><mtd></mtd></mtr>");
        }
        for row in rows {
            self.out.push_str("<mtr>");
            for cell in row {
                self.out.push_str("<mtd>");
                self.arg(cell, variant);
                self.out.push_str("</mtd>");
            }
            for _ in row.len()..cols {
                self.out.push_str("<mtd></mtd>");
            }
            self.out.push_str("</mtr>");
        }
        self.out.push_str("</mtable>");
        if fenced {
            if let Some(r) = close {
                self.mo(r, None, true);
            }
            self.out.push_str("</mrow>");
        }
    }

    fn node(&mut self, node: &Node, variant: Option<Variant>) {
        use NodeKind::*;
        match &node.kind {
            Row(items) => self.mrow(items, variant),
            Run(t) => self.run(t, variant),
            Sym(t) => self.sym(t, variant),
            Space(em) => {
                let _ = write!(
                    self.out,
                    r#"<mspace width="{}em"></mspace>"#,
                    format_em(*em)
                );
            }
            Frac { style, num, den } => {
                let (open, close) = match style {
                    FracStyle::Display => (r#"<mstyle displaystyle="true">"#, "</mstyle>"),
                    FracStyle::Text => (r#"<mstyle displaystyle="false">"#, "</mstyle>"),
                    FracStyle::Binom => (r#"<mrow><mo stretchy="true">(</mo>"#, ""),
                    _ => ("", ""),
                };
                self.out.push_str(open);
                match style {
                    FracStyle::NoBar | FracStyle::Binom => {
                        self.out.push_str(r#"<mfrac linethickness="0">"#);
                    }
                    _ => self.out.push_str("<mfrac>"),
                }
                self.arg(num, variant);
                self.arg(den, variant);
                self.out.push_str("</mfrac>");
                if *style == FracStyle::Binom {
                    self.out.push_str(r#"<mo stretchy="true">)</mo></mrow>"#);
                } else {
                    self.out.push_str(close);
                }
            }
            Sqrt { degree, body } => match degree {
                Some(d) => {
                    self.out.push_str("<mroot>");
                    self.arg(body, variant);
                    self.arg(d, variant);
                    self.out.push_str("</mroot>");
                }
                None => {
                    self.out.push_str("<msqrt>");
                    self.arg(body, variant);
                    self.out.push_str("</msqrt>");
                }
            },
            Scripts {
                base,
                sub,
                sup,
                limits,
            } => self.scripts(base, sub.as_deref(), sup.as_deref(), *limits, variant),
            Nary { text, limits } => self.nary(text, *limits, None, None, variant),
            Func { name, limits } => self.func(name, *limits, None, None, variant),
            Delimited { left, right, parts } => {
                self.out.push_str("<mrow>");
                if let Some(l) = left {
                    self.mo(l, None, true);
                }
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        self.mo("|", None, true);
                    }
                    self.arg(part, variant);
                }
                if let Some(r) = right {
                    self.mo(r, None, true);
                }
                self.out.push_str("</mrow>");
            }
            Accent { text, body } => {
                self.out.push_str(r#"<mover accent="true">"#);
                self.arg(body, variant);
                let _ = write!(
                    self.out,
                    "<mo>{}</mo></mover>",
                    escape_text(accent_char(text))
                );
            }
            Bar { pos, body } => {
                let (tag, attr, line) = match pos {
                    Pos::Top => ("mover", "accent", "\u{203e}"),
                    Pos::Bot => ("munder", "accentunder", "\u{5f}"),
                };
                let _ = write!(self.out, r#"<{tag} {attr}="true">"#);
                self.arg(body, variant);
                let _ = write!(self.out, r#"<mo stretchy="true">{line}</mo></{tag}>"#);
            }
            GroupChr { text, pos, body } => self.group_chr(text, *pos, body, variant),
            LimPos {
                pos,
                annotation,
                body,
            } => {
                let tag = match pos {
                    Pos::Top => "mover",
                    Pos::Bot => "munder",
                };
                let _ = write!(self.out, "<{tag}>");
                self.arg(body, variant);
                self.arg(annotation, variant);
                let _ = write!(self.out, "</{tag}>");
            }
            XArrow { text, above, below } => {
                let text = text.clone();
                self.attach(
                    |w| {
                        let _ = write!(w.out, r#"<mo stretchy="true">{}</mo>"#, escape_text(&text));
                    },
                    below.as_deref(),
                    above.as_deref(),
                    true,
                    variant,
                );
            }
            Style { variant: v, body } => self.arg(body, Some(*v)),
            Text { variant: v, text } => {
                // Token elements trim edge whitespace; TeX keeps it.
                let mut s = styled(text, Some(*v));
                if s.starts_with(' ') {
                    s.replace_range(..1, "\u{a0}");
                }
                if s.ends_with(' ') {
                    let n = s.len();
                    s.replace_range(n - 1.., "\u{a0}");
                }
                let _ = write!(self.out, "<mtext>{}</mtext>", escape_text(&s));
            }
            Phantom { h, v, body } => {
                let (open, close) = match (h, v) {
                    (true, true) => ("", ""),
                    (true, false) => (r#"<mpadded height="0" depth="0">"#, "</mpadded>"),
                    (false, true) => (r#"<mpadded width="0">"#, "</mpadded>"),
                    (false, false) => ("", ""),
                };
                self.out.push_str(open);
                self.out.push_str("<mphantom>");
                self.arg(body, variant);
                self.out.push_str("</mphantom>");
                self.out.push_str(close);
            }
            Cancel(body) => {
                self.out
                    .push_str(r#"<menclose notation="updiagonalstrike">"#);
                self.arg(body, variant);
                self.out.push_str("</menclose>");
            }
            Color { color, body } => {
                let _ = write!(
                    self.out,
                    r#"<mstyle mathcolor="{}">"#,
                    escape_attr(&css_color(color))
                );
                self.arg(body, variant);
                self.out.push_str("</mstyle>");
            }
            Matrix {
                layout,
                delims,
                rows,
            } => self.matrix(*layout, delims.as_ref(), rows, variant),
            Break => {
                // Handled at the root; inside environments normalization
                // consumed it.
            }
            Error { verbatim, .. } => {
                let _ = write!(
                    self.out,
                    "<merror><mtext>{}</mtext></merror>",
                    escape_text(verbatim)
                );
            }
        }
    }
}

/// Whether [`Writer::node`] emits exactly one element for `node`. Rows
/// are handled by the caller; a function name is followed by its
/// U+2061 application operator, so it (and scripts on it) emit two.
fn emits_one_element(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Row(items) => items.len() != 1 || emits_one_element(&items[0]),
        NodeKind::Run(text) => split_run(text, None).len() == 1,
        NodeKind::Func { .. } => false,
        NodeKind::Scripts { base, .. } => !matches!(unwrap_row(base).kind, NodeKind::Func { .. }),
        NodeKind::Style { body, .. } => emits_one_element(body),
        NodeKind::Break => false,
        _ => true,
    }
}

/// Look through single-item rows (brace groups around one node).
fn unwrap_row(node: &Node) -> &Node {
    match &node.kind {
        NodeKind::Row(items) if items.len() == 1 => unwrap_row(&items[0]),
        _ => node,
    }
}

fn split_at_breaks(items: &[Node]) -> Vec<&[Node]> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, item) in items.iter().enumerate() {
        if matches!(item.kind, NodeKind::Break) {
            lines.push(&items[start..i]);
            start = i + 1;
        }
    }
    lines.push(&items[start..]);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styled_char_covers_every_variant_and_hole() {
        use Variant::*;
        assert_eq!(styled_char('A', Bold), '𝐀');
        assert_eq!(styled_char('z', Bold), '𝐳');
        assert_eq!(styled_char('7', Bold), '𝟕');
        assert_eq!(styled_char('β', Bold), '𝛃');
        assert_eq!(styled_char('Σ', Bold), '𝚺');
        assert_eq!(styled_char('a', Italic), '𝑎');
        assert_eq!(styled_char('h', Italic), 'ℎ');
        assert_eq!(styled_char('β', BoldItalic), '𝜷');
        assert_eq!(styled_char('P', Script), '𝒫');
        for (c, expected) in [
            ('B', 'ℬ'),
            ('E', 'ℰ'),
            ('F', 'ℱ'),
            ('H', 'ℋ'),
            ('I', 'ℐ'),
            ('L', 'ℒ'),
            ('M', 'ℳ'),
            ('R', 'ℛ'),
            ('e', 'ℯ'),
            ('g', 'ℊ'),
            ('o', 'ℴ'),
        ] {
            assert_eq!(styled_char(c, Script), expected, "script {c}");
        }
        assert_eq!(styled_char('A', Fraktur), '𝔄');
        for (c, expected) in [('C', 'ℭ'), ('H', 'ℌ'), ('I', 'ℑ'), ('R', 'ℜ'), ('Z', 'ℨ')]
        {
            assert_eq!(styled_char(c, Fraktur), expected, "fraktur {c}");
        }
        assert_eq!(styled_char('A', DoubleStruck), '𝔸');
        assert_eq!(styled_char('1', DoubleStruck), '𝟙');
        for (c, expected) in [
            ('C', 'ℂ'),
            ('H', 'ℍ'),
            ('N', 'ℕ'),
            ('P', 'ℙ'),
            ('Q', 'ℚ'),
            ('R', 'ℝ'),
            ('Z', 'ℤ'),
        ] {
            assert_eq!(styled_char(c, DoubleStruck), expected, "double-struck {c}");
        }
        assert_eq!(styled_char('A', Sans), '𝖠');
        assert_eq!(styled_char('0', Sans), '𝟢');
        assert_eq!(styled_char('a', Mono), '𝚊');
        assert_eq!(styled_char('9', Mono), '𝟿');
        // No glyph for the variant: unchanged.
        assert_eq!(styled_char('3', Script), '3');
        assert_eq!(styled_char('+', Bold), '+');
        assert_eq!(styled_char('x', Roman), 'x');
    }

    #[test]
    fn accents_use_spacing_forms() {
        assert_eq!(accent_char("\u{302}"), "ˆ");
        assert_eq!(accent_char("\u{20d7}"), "→");
        assert_eq!(accent_char("\u{20db}"), "\u{20db}");
    }

    #[test]
    fn colors_become_css() {
        assert_eq!(css_color("red"), "red");
        assert_eq!(css_color("#FF00aa"), "#ff00aa");
        assert_eq!(css_color("00FF00"), "#00ff00");
    }
}
