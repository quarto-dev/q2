/*
 * omml.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! `MathAst` → OMML (Office Math Markup Language, ECMA-376 Part 1 §22.1),
//! the math vocabulary of `.docx`.
//!
//! The writer is total: every node has a rendering, an `Error` node becomes
//! a literal text run, so the caller (pampa's docx writer) decides from
//! [`Node::has_errors`] whether to emit this or fall back to verbatim TeX.
//! Children are emitted in schema order by construction; every fixture's
//! output is validated against the vendored `shared-math.xsd` in
//! `tests/integration/omml.rs`.
//!
//! Conventions (from the plan's Phase 1 design):
//!
//! - `m:oMath` for inline math, `m:oMathPara` > `m:oMath` for display;
//! - runs are `m:r` > `m:t`; text-mode content gets `m:nor`, styles fold
//!   into `m:sty` / `m:scr` on the runs they contain, colors into `w:color`;
//! - a big operator is an `m:nary` with an empty body: TeX gives a summand
//!   no scope, so what follows stays a sibling; missing limits are hidden,
//!   not omitted (the schema requires `m:sub` and `m:sup`);
//! - a function name is an `m:func` with an empty body, its limits as
//!   `m:limLow`/`m:sSub` on the name;
//! - `\left…\right` is `m:d` with explicit fences (`""` for `.`);
//! - matrices are `m:m` inside `m:d` when fenced; `aligned`, `cases` and
//!   top-level `\\` become `m:eqArr`, with literal `&` as alignment marks
//!   (§22.1.2.34).

use std::fmt::Write;

use crate::ast::{EnvLayout, FracStyle, LimLoc, Node, NodeKind, Pos, Variant};
use crate::normalize::{Mode, Normalized};

const M_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// Big operators whose limits sit beside them even in display math.
const SIDE_LIMIT_OPERATORS: &str = "∫∬∭∮∯∰⨌";

/// OMML for embedding in a document that already declares the `m:` (and
/// `w:`) namespace.
pub fn to_omml(n: &Normalized) -> String {
    let mut w = Writer::new(n.mode);
    w.document(&n.root);
    w.out
}

/// OMML as a standalone XML fragment with the namespaces declared on the
/// root element (what the schema tests validate).
pub fn to_omml_document(n: &Normalized) -> String {
    let body = to_omml(n);
    let root_end = body.find('>').expect("root element");
    let decl = format!(r#" xmlns:m="{M_NS}" xmlns:w="{W_NS}""#);
    format!("{}{}{}", &body[..root_end], decl, &body[root_end..])
}

/// Run properties inherited from enclosing `Style` / `Text` / `Color` nodes.
#[derive(Debug, Clone, Default, PartialEq)]
struct RunStyle {
    /// `m:sty`: `p` plain, `b`, `i`, `bi`.
    sty: Option<&'static str>,
    /// `m:scr`: `roman`, `script`, `fraktur`, `double-struck`, `sans-serif`, `monospace`.
    scr: Option<&'static str>,
    /// `m:nor`: text-mode run.
    nor: bool,
    /// Literal (error) run.
    lit: bool,
    /// `w:color` hex.
    color: Option<String>,
    /// `w:b` / `w:i` for text-mode runs (`m:nor` excludes `m:sty`).
    w_bold: bool,
    w_italic: bool,
}

impl RunStyle {
    fn with_variant(&self, variant: Variant) -> RunStyle {
        let mut s = self.clone();
        match variant {
            Variant::Roman => s.sty = Some("p"),
            Variant::Italic => s.sty = Some("i"),
            Variant::Bold => s.sty = Some("b"),
            Variant::BoldItalic => s.sty = Some("bi"),
            Variant::DoubleStruck => {
                s.scr = Some("double-struck");
                s.sty = Some("p");
            }
            Variant::Script => {
                s.scr = Some("script");
                s.sty = Some("p");
            }
            Variant::Fraktur => {
                s.scr = Some("fraktur");
                s.sty = Some("p");
            }
            Variant::Sans => s.scr = Some("sans-serif"),
            Variant::Mono => s.scr = Some("monospace"),
        }
        s
    }
}

struct Writer {
    mode: Mode,
    out: String,
}

fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

/// Unicode space of the closest width; `None` drops the space.
fn space_char(em: f32) -> Option<&'static str> {
    if em <= 0.0 {
        return None;
    }
    Some(if em < 0.2 {
        "\u{2009}" // thin
    } else if em < 0.25 {
        "\u{2005}" // four-per-em
    } else if em < 0.3 {
        "\u{2004}" // three-per-em
    } else if em < 0.6 {
        "\u{2002}" // en
    } else if em < 1.5 {
        "\u{2003}" // em
    } else {
        "\u{2003}\u{2003}"
    })
}

/// Named colors LaTeX documents commonly use; hex passes through.
fn color_hex(name: &str) -> String {
    let name = name.trim();
    let hex = match name.to_ascii_lowercase().as_str() {
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
    };
    hex.to_string()
}

impl Writer {
    fn new(mode: Mode) -> Writer {
        Writer {
            mode,
            out: String::new(),
        }
    }

    fn document(&mut self, root: &Node) {
        let items: &[Node] = match &root.kind {
            NodeKind::Row(items) => items,
            _ => std::slice::from_ref(root),
        };
        match self.mode {
            Mode::Display => self.out.push_str("<m:oMathPara><m:oMath>"),
            Mode::Inline => self.out.push_str("<m:oMath>"),
        }
        let style = RunStyle::default();
        if items.iter().any(|n| matches!(n.kind, NodeKind::Break)) {
            // Top-level line breaks: one equation-array row per line.
            self.out.push_str("<m:eqArr>");
            for line in split_at_breaks(items) {
                self.out.push_str("<m:e>");
                self.items(line, &style);
                self.out.push_str("</m:e>");
            }
            self.out.push_str("</m:eqArr>");
        } else {
            self.items(items, &style);
        }
        match self.mode {
            Mode::Display => self.out.push_str("</m:oMath></m:oMathPara>"),
            Mode::Inline => self.out.push_str("</m:oMath>"),
        }
    }

    fn items(&mut self, items: &[Node], style: &RunStyle) {
        for item in items {
            self.node(item, style);
        }
    }

    /// A math argument: `<tag>…</tag>`, content or empty.
    fn arg(&mut self, tag: &str, node: Option<&Node>, style: &RunStyle) {
        match node {
            Some(n) => {
                let _ = write!(self.out, "<{tag}>");
                self.node(n, style);
                let _ = write!(self.out, "</{tag}>");
            }
            None => {
                let _ = write!(self.out, "<{tag}/>");
            }
        }
    }

    fn run(&mut self, text: &str, style: &RunStyle) {
        if text.is_empty() {
            return;
        }
        self.out.push_str("<m:r>");
        let has_m_props = style.lit || style.nor || style.scr.is_some() || style.sty.is_some();
        if has_m_props {
            self.out.push_str("<m:rPr>");
            if style.lit {
                self.out.push_str(r#"<m:lit m:val="1"/>"#);
            }
            if style.nor {
                self.out.push_str("<m:nor/>");
            }
            if let Some(scr) = style.scr {
                let _ = write!(self.out, r#"<m:scr m:val="{scr}"/>"#);
            }
            if let Some(sty) = style.sty {
                let _ = write!(self.out, r#"<m:sty m:val="{sty}"/>"#);
            }
            self.out.push_str("</m:rPr>");
        }
        if style.color.is_some() || style.w_bold || style.w_italic {
            // w:rPr child order: b, i, color (CT_RPr sequence).
            self.out.push_str("<w:rPr>");
            if style.w_bold {
                self.out.push_str("<w:b/>");
            }
            if style.w_italic {
                self.out.push_str("<w:i/>");
            }
            if let Some(color) = &style.color {
                let _ = write!(self.out, r#"<w:color w:val="{color}"/>"#);
            }
            self.out.push_str("</w:rPr>");
        }
        let preserve = text.starts_with(' ') || text.ends_with(' ');
        if preserve {
            let _ = write!(
                self.out,
                r#"<m:t xml:space="preserve">{}</m:t>"#,
                escape_text(text)
            );
        } else {
            let _ = write!(self.out, "<m:t>{}</m:t>", escape_text(text));
        }
        self.out.push_str("</m:r>");
    }

    fn lim_loc(&self, limits: LimLoc, operator: &str) -> &'static str {
        match limits {
            LimLoc::UndOvr => "undOvr",
            LimLoc::SubSup => "subSup",
            LimLoc::Auto => {
                let side = operator
                    .chars()
                    .next()
                    .is_some_and(|c| SIDE_LIMIT_OPERATORS.contains(c));
                if self.mode == Mode::Display && !side {
                    "undOvr"
                } else {
                    "subSup"
                }
            }
        }
    }

    fn node(&mut self, node: &Node, style: &RunStyle) {
        use NodeKind::*;
        match &node.kind {
            Row(items) => self.items(items, style),
            Run(t) | Sym(t) => self.run(t, style),
            Space(em) => {
                if let Some(s) = space_char(*em) {
                    self.run(s, style);
                }
            }
            Frac {
                style: fstyle,
                num,
                den,
            } => {
                let fenced = *fstyle == FracStyle::Binom;
                if fenced {
                    self.out.push_str(
                        r#"<m:d><m:dPr><m:begChr m:val="("/><m:endChr m:val=")"/></m:dPr><m:e>"#,
                    );
                }
                self.out.push_str("<m:f>");
                if matches!(fstyle, FracStyle::NoBar | FracStyle::Binom) {
                    self.out
                        .push_str(r#"<m:fPr><m:type m:val="noBar"/></m:fPr>"#);
                }
                self.arg("m:num", Some(num), style);
                self.arg("m:den", Some(den), style);
                self.out.push_str("</m:f>");
                if fenced {
                    self.out.push_str("</m:e></m:d>");
                }
            }
            Sqrt { degree, body } => {
                self.out.push_str("<m:rad>");
                match degree {
                    Some(d) => {
                        self.arg("m:deg", Some(d), style);
                    }
                    None => {
                        self.out
                            .push_str(r#"<m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/>"#);
                    }
                }
                self.arg("m:e", Some(body), style);
                self.out.push_str("</m:rad>");
            }
            Scripts {
                base,
                sub,
                sup,
                limits,
            } => self.scripts(base, sub.as_deref(), sup.as_deref(), *limits, style),
            Nary { text, limits } => self.nary(text, *limits, None, None, style),
            Func { name, limits } => self.func(name, *limits, None, None, style),
            Delimited { left, right, parts } => {
                self.out.push_str("<m:d><m:dPr>");
                let _ = write!(
                    self.out,
                    r#"<m:begChr m:val="{}"/>"#,
                    escape_attr(left.as_deref().unwrap_or(""))
                );
                if parts.len() > 1 {
                    self.out.push_str(r#"<m:sepChr m:val="|"/>"#);
                }
                let _ = write!(
                    self.out,
                    r#"<m:endChr m:val="{}"/>"#,
                    escape_attr(right.as_deref().unwrap_or(""))
                );
                self.out.push_str("</m:dPr>");
                if parts.is_empty() {
                    self.out.push_str("<m:e/>");
                }
                for part in parts {
                    self.arg("m:e", Some(part), style);
                }
                self.out.push_str("</m:d>");
            }
            Accent { text, body } => {
                let _ = write!(
                    self.out,
                    r#"<m:acc><m:accPr><m:chr m:val="{}"/></m:accPr>"#,
                    escape_attr(text)
                );
                self.arg("m:e", Some(body), style);
                self.out.push_str("</m:acc>");
            }
            Bar { pos, body } => {
                let _ = write!(
                    self.out,
                    r#"<m:bar><m:barPr><m:pos m:val="{}"/></m:barPr>"#,
                    pos_val(*pos)
                );
                self.arg("m:e", Some(body), style);
                self.out.push_str("</m:bar>");
            }
            GroupChr { text, pos, body } => self.group_chr(text, *pos, body, style),
            LimPos {
                pos,
                annotation,
                body,
            } => self.lim(*pos, body, annotation, style),
            XArrow { text, above, below } => {
                // The arrow is the base; annotations stack above and below.
                let arrow = Node::new(Sym(text.clone()), node.span.clone());
                match (above.as_deref(), below.as_deref()) {
                    (Some(a), None) => self.lim(Pos::Top, &arrow, a, style),
                    (None, Some(b)) => self.lim(Pos::Bot, &arrow, b, style),
                    (Some(a), Some(b)) => {
                        self.out.push_str("<m:limLow><m:e>");
                        self.lim(Pos::Top, &arrow, a, style);
                        self.out.push_str("</m:e>");
                        self.arg("m:lim", Some(b), style);
                        self.out.push_str("</m:limLow>");
                    }
                    (None, None) => self.run(text, style),
                }
            }
            Style { variant, body } => {
                let inner = style.with_variant(*variant);
                self.node(body, &inner);
            }
            Text { variant, text } => {
                // A text-mode run: `m:nor`, which excludes `m:sty`/`m:scr`, so
                // weight and slant go through Word's run properties instead.
                let mut inner = style.clone();
                inner.nor = true;
                inner.sty = None;
                inner.scr = None;
                match variant {
                    Variant::Bold => inner.w_bold = true,
                    Variant::Italic => inner.w_italic = true,
                    Variant::BoldItalic => {
                        inner.w_bold = true;
                        inner.w_italic = true;
                    }
                    _ => {}
                }
                self.run(text, &inner);
            }
            Phantom { h, v, body } => {
                self.out
                    .push_str(r#"<m:phant><m:phantPr><m:show m:val="0"/>"#);
                if !h {
                    self.out.push_str(r#"<m:zeroWid m:val="1"/>"#);
                }
                if !v {
                    self.out
                        .push_str(r#"<m:zeroAsc m:val="1"/><m:zeroDesc m:val="1"/>"#);
                }
                self.out.push_str("</m:phantPr>");
                self.arg("m:e", Some(body), style);
                self.out.push_str("</m:phant>");
            }
            Cancel(body) => {
                self.out.push_str(
                    r#"<m:borderBox><m:borderBoxPr><m:hideTop m:val="1"/><m:hideBot m:val="1"/><m:hideLeft m:val="1"/><m:hideRight m:val="1"/><m:strikeBLTR m:val="1"/></m:borderBoxPr>"#,
                );
                self.arg("m:e", Some(body), style);
                self.out.push_str("</m:borderBox>");
            }
            Color { color, body } => {
                let mut inner = style.clone();
                inner.color = Some(color_hex(color));
                self.node(body, &inner);
            }
            Matrix {
                layout,
                delims,
                rows,
            } => self.matrix(*layout, delims.as_ref(), rows, style),
            Break => {
                // Only meaningful at the top level (handled in `document`)
                // or inside an environment (consumed by normalization).
            }
            Error { verbatim, .. } => {
                // A literal text run; `m:nor` excludes `m:sty`/`m:scr`.
                let mut inner = style.clone();
                inner.lit = true;
                inner.nor = true;
                inner.sty = None;
                inner.scr = None;
                self.run(verbatim, &inner);
            }
        }
    }

    fn scripts(
        &mut self,
        base: &Node,
        sub: Option<&Node>,
        sup: Option<&Node>,
        limits: LimLoc,
        style: &RunStyle,
    ) {
        // Operators and functions absorb their scripts as limits.
        match &unwrap_row(base).kind {
            NodeKind::Nary { text, .. } => return self.nary(text, limits, sub, sup, style),
            NodeKind::Func { name, .. } => return self.func(name, limits, sub, sup, style),
            NodeKind::GroupChr {
                text,
                pos: Pos::Bot,
                body,
            } if sub.is_some() && sup.is_none() => {
                return self.group_chr_with_lim(text, Pos::Bot, body, sub.unwrap(), style);
            }
            NodeKind::GroupChr {
                text,
                pos: Pos::Top,
                body,
            } if sup.is_some() && sub.is_none() => {
                return self.group_chr_with_lim(text, Pos::Top, body, sup.unwrap(), style);
            }
            _ => {}
        }
        match (sub, sup) {
            (Some(sub), Some(sup)) => {
                self.out.push_str("<m:sSubSup>");
                self.arg("m:e", Some(base), style);
                self.arg("m:sub", Some(sub), style);
                self.arg("m:sup", Some(sup), style);
                self.out.push_str("</m:sSubSup>");
            }
            (Some(sub), None) => {
                self.out.push_str("<m:sSub>");
                self.arg("m:e", Some(base), style);
                self.arg("m:sub", Some(sub), style);
                self.out.push_str("</m:sSub>");
            }
            (None, Some(sup)) => {
                self.out.push_str("<m:sSup>");
                self.arg("m:e", Some(base), style);
                self.arg("m:sup", Some(sup), style);
                self.out.push_str("</m:sSup>");
            }
            (None, None) => self.node(base, style),
        }
    }

    fn nary(
        &mut self,
        text: &str,
        limits: LimLoc,
        sub: Option<&Node>,
        sup: Option<&Node>,
        style: &RunStyle,
    ) {
        let loc = self.lim_loc(limits, text);
        let _ = write!(
            self.out,
            r#"<m:nary><m:naryPr><m:chr m:val="{}"/><m:limLoc m:val="{loc}"/>"#,
            escape_attr(text)
        );
        if sub.is_none() {
            self.out.push_str(r#"<m:subHide m:val="1"/>"#);
        }
        if sup.is_none() {
            self.out.push_str(r#"<m:supHide m:val="1"/>"#);
        }
        self.out.push_str("</m:naryPr>");
        self.arg("m:sub", sub, style);
        self.arg("m:sup", sup, style);
        self.out.push_str("<m:e/></m:nary>");
    }

    fn func(
        &mut self,
        name: &str,
        limits: LimLoc,
        sub: Option<&Node>,
        sup: Option<&Node>,
        style: &RunStyle,
    ) {
        let mut name_style = style.clone();
        name_style.sty = Some("p");
        self.out.push_str("<m:func><m:fName>");
        let under_over = self.lim_loc(limits, name) == "undOvr";
        match (sub, sup, under_over) {
            (None, None, _) => self.run(name, &name_style),
            (Some(sub), None, true) => {
                self.out.push_str("<m:limLow><m:e>");
                self.run(name, &name_style);
                self.out.push_str("</m:e>");
                self.arg("m:lim", Some(sub), style);
                self.out.push_str("</m:limLow>");
            }
            (None, Some(sup), true) => {
                self.out.push_str("<m:limUpp><m:e>");
                self.run(name, &name_style);
                self.out.push_str("</m:e>");
                self.arg("m:lim", Some(sup), style);
                self.out.push_str("</m:limUpp>");
            }
            (Some(sub), Some(sup), true) => {
                self.out.push_str("<m:limUpp><m:e><m:limLow><m:e>");
                self.run(name, &name_style);
                self.out.push_str("</m:e>");
                self.arg("m:lim", Some(sub), style);
                self.out.push_str("</m:limLow></m:e>");
                self.arg("m:lim", Some(sup), style);
                self.out.push_str("</m:limUpp>");
            }
            (sub, sup, false) => {
                let tag = match (sub.is_some(), sup.is_some()) {
                    (true, true) => "m:sSubSup",
                    (true, false) => "m:sSub",
                    _ => "m:sSup",
                };
                let _ = write!(self.out, "<{tag}><m:e>");
                self.run(name, &name_style);
                self.out.push_str("</m:e>");
                if let Some(s) = sub {
                    self.arg("m:sub", Some(s), style);
                }
                if let Some(s) = sup {
                    self.arg("m:sup", Some(s), style);
                }
                let _ = write!(self.out, "</{tag}>");
            }
        }
        self.out.push_str("</m:fName><m:e/></m:func>");
    }

    fn group_chr(&mut self, text: &str, pos: Pos, body: &Node, style: &RunStyle) {
        let _ = write!(
            self.out,
            r#"<m:groupChr><m:groupChrPr><m:chr m:val="{}"/><m:pos m:val="{}"/>"#,
            escape_attr(text),
            pos_val(pos)
        );
        if pos == Pos::Top {
            self.out.push_str(r#"<m:vertJc m:val="bot"/>"#);
        }
        self.out.push_str("</m:groupChrPr>");
        self.arg("m:e", Some(body), style);
        self.out.push_str("</m:groupChr>");
    }

    /// `\underbrace{…}_{n}` / `\overbrace{…}^{n}`: the script is the limit
    /// of the braced group.
    fn group_chr_with_lim(
        &mut self,
        text: &str,
        pos: Pos,
        body: &Node,
        lim: &Node,
        style: &RunStyle,
    ) {
        let tag = if pos == Pos::Top {
            "m:limUpp"
        } else {
            "m:limLow"
        };
        let _ = write!(self.out, "<{tag}><m:e>");
        self.group_chr(text, pos, body, style);
        self.out.push_str("</m:e>");
        self.arg("m:lim", Some(lim), style);
        let _ = write!(self.out, "</{tag}>");
    }

    /// `m:limUpp` / `m:limLow`: `annotation` over or under `base`.
    fn lim(&mut self, pos: Pos, base: &Node, annotation: &Node, style: &RunStyle) {
        let tag = if pos == Pos::Top {
            "m:limUpp"
        } else {
            "m:limLow"
        };
        let _ = write!(self.out, "<{tag}>");
        self.arg("m:e", Some(base), style);
        self.arg("m:lim", Some(annotation), style);
        let _ = write!(self.out, "</{tag}>");
    }

    fn matrix(
        &mut self,
        layout: EnvLayout,
        delims: Option<&(String, String)>,
        rows: &[Vec<Node>],
        style: &RunStyle,
    ) {
        match layout {
            EnvLayout::Matrix => {
                if let Some((l, r)) = delims {
                    let _ = write!(
                        self.out,
                        r#"<m:d><m:dPr><m:begChr m:val="{}"/><m:endChr m:val="{}"/></m:dPr><m:e>"#,
                        escape_attr(l),
                        escape_attr(r)
                    );
                }
                let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
                let _ = write!(
                    self.out,
                    r#"<m:m><m:mPr><m:mcs><m:mc><m:mcPr><m:count m:val="{cols}"/><m:mcJc m:val="center"/></m:mcPr></m:mc></m:mcs></m:mPr>"#
                );
                if rows.is_empty() {
                    self.out.push_str("<m:mr><m:e/></m:mr>");
                }
                for row in rows {
                    self.out.push_str("<m:mr>");
                    for cell in row {
                        self.arg("m:e", Some(cell), style);
                    }
                    for _ in row.len()..cols {
                        self.out.push_str("<m:e/>");
                    }
                    self.out.push_str("</m:mr>");
                }
                self.out.push_str("</m:m>");
                if delims.is_some() {
                    self.out.push_str("</m:e></m:d>");
                }
            }
            EnvLayout::Cases | EnvLayout::Rcases => {
                let (l, r) = if layout == EnvLayout::Cases {
                    ("{", "")
                } else {
                    ("", "}")
                };
                let _ = write!(
                    self.out,
                    r#"<m:d><m:dPr><m:begChr m:val="{l}"/><m:endChr m:val="{r}"/></m:dPr><m:e>"#
                );
                self.eq_arr(rows, true, style);
                self.out.push_str("</m:e></m:d>");
            }
            EnvLayout::Aligned => self.eq_arr(rows, true, style),
            EnvLayout::Gathered => self.eq_arr(rows, false, style),
        }
    }

    /// `m:eqArr`: one `m:e` per row; cells joined by a literal `&`, the
    /// alignment mark of §22.1.2.34, when `align` is set.
    fn eq_arr(&mut self, rows: &[Vec<Node>], align: bool, style: &RunStyle) {
        self.out.push_str("<m:eqArr>");
        if rows.is_empty() {
            self.out.push_str("<m:e/>");
        }
        for row in rows {
            self.out.push_str("<m:e>");
            for (i, cell) in row.iter().enumerate() {
                if i > 0 && align {
                    self.run("&", style);
                }
                self.node(cell, style);
            }
            self.out.push_str("</m:e>");
        }
        self.out.push_str("</m:eqArr>");
    }
}

fn pos_val(pos: Pos) -> &'static str {
    match pos {
        Pos::Top => "top",
        Pos::Bot => "bot",
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
