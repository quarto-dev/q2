/*
 * lua/show.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Haskell-`show`-style rendering of Pandoc AST values, matching what
 * real Pandoc's Lua API produces for `tostring(element)` and
 * `tostring(Inlines/Blocks)` (pandoc-lua-marshal renders userdata via
 * the derived Haskell `Show` instances of pandoc-types).
 *
 * The exact formats below were probed against pandoc 3.9.0.2
 * (`pandoc lua -e 'print(tostring(...))'`) and are pinned by the unit
 * tests at the bottom and by the vendored conformance suite
 * (tests/lua-conformance/, strand bd-55mb0rjz).
 *
 * q2 extension nodes (Insert/Delete/…, Shortcode, custom nodes) have
 * no Pandoc counterpart; they get stable pandoc-style spellings so
 * that tostring-based comparisons remain value-deterministic.
 */

use crate::pandoc::{
    Alignment, Attr, Block, Caption, Cell, Citation, CitationMode, ColWidth, Inline,
    ListAttributes, ListNumberDelim, ListNumberStyle, MathType, QuoteType, Row, TableBody,
    TableFoot, TableHead,
};

/// Haskell `show` for a string: double-quoted, `\"`/`\\` escaped,
/// common control characters as named escapes, everything else
/// non-printable-ASCII as decimal escapes (with Haskell's `\&`
/// separator when a digit follows a numeric escape).
pub fn show_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    let mut prev_numeric_escape = false;
    for c in s.chars() {
        if prev_numeric_escape && c.is_ascii_digit() {
            out.push_str("\\&");
        }
        prev_numeric_escape = false;
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\x07' => out.push_str("\\a"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\x0b' => out.push_str("\\v"),
            c if (' '..='\x7e').contains(&c) => out.push(c),
            c => {
                out.push_str(&format!("\\{}", c as u32));
                prev_numeric_escape = true;
            }
        }
    }
    out.push('"');
    out
}

/// Haskell `show` for a Double: always carries a decimal point.
fn show_double(x: f64) -> String {
    if x.fract() == 0.0 && x.is_finite() {
        format!("{x:.1}")
    } else {
        format!("{x}")
    }
}

fn show_string_list(items: &[String]) -> String {
    let shown: Vec<String> = items.iter().map(|s| show_string(s)).collect();
    format!("[{}]", shown.join(","))
}

/// `("id",["c1","c2"],[("k","v")])`
pub fn show_attr(attr: &Attr) -> String {
    let kvs: Vec<String> = attr
        .2
        .iter()
        .map(|(k, v)| format!("({},{})", show_string(k), show_string(v)))
        .collect();
    format!(
        "({},{},[{}])",
        show_string(&attr.0),
        show_string_list(&attr.1),
        kvs.join(",")
    )
}

fn show_target(target: &(String, String)) -> String {
    format!("({},{})", show_string(&target.0), show_string(&target.1))
}

fn show_math_type(mt: &MathType) -> &'static str {
    match mt {
        MathType::InlineMath => "InlineMath",
        MathType::DisplayMath => "DisplayMath",
    }
}

fn show_quote_type(qt: &QuoteType) -> &'static str {
    match qt {
        QuoteType::SingleQuote => "SingleQuote",
        QuoteType::DoubleQuote => "DoubleQuote",
    }
}

fn show_alignment(a: &Alignment) -> &'static str {
    match a {
        Alignment::Left => "AlignLeft",
        Alignment::Center => "AlignCenter",
        Alignment::Right => "AlignRight",
        Alignment::Default => "AlignDefault",
    }
}

fn show_col_width(w: &ColWidth) -> String {
    match w {
        ColWidth::Default => "ColWidthDefault".to_string(),
        ColWidth::Percentage(p) => format!("ColWidth {}", show_double(*p)),
    }
}

fn show_list_number_style(s: &ListNumberStyle) -> &'static str {
    match s {
        ListNumberStyle::Default => "DefaultStyle",
        ListNumberStyle::Example => "Example",
        ListNumberStyle::Decimal => "Decimal",
        ListNumberStyle::LowerRoman => "LowerRoman",
        ListNumberStyle::UpperRoman => "UpperRoman",
        ListNumberStyle::LowerAlpha => "LowerAlpha",
        ListNumberStyle::UpperAlpha => "UpperAlpha",
    }
}

fn show_list_number_delim(d: &ListNumberDelim) -> &'static str {
    match d {
        ListNumberDelim::Default => "DefaultDelim",
        ListNumberDelim::Period => "Period",
        ListNumberDelim::OneParen => "OneParen",
        ListNumberDelim::TwoParens => "TwoParens",
    }
}

/// `(3,Decimal,Period)`
pub fn show_list_attributes(attr: &ListAttributes) -> String {
    format!(
        "({},{},{})",
        attr.0,
        show_list_number_style(&attr.1),
        show_list_number_delim(&attr.2)
    )
}

fn show_citation_mode(m: &CitationMode) -> &'static str {
    match m {
        CitationMode::AuthorInText => "AuthorInText",
        CitationMode::SuppressAuthor => "SuppressAuthor",
        CitationMode::NormalCitation => "NormalCitation",
    }
}

/// Haskell record syntax, as pandoc-types derives it.
pub fn show_citation(c: &Citation) -> String {
    format!(
        "Citation {{citationId = {}, citationPrefix = {}, citationSuffix = {}, \
         citationMode = {}, citationNoteNum = {}, citationHash = {}}}",
        show_string(&c.id),
        show_inlines(&c.prefix),
        show_inlines(&c.suffix),
        show_citation_mode(&c.mode),
        c.note_num,
        c.hash
    )
}

/// `Caption Nothing [Plain [Str "x"]]` / `Caption (Just [Str "s"]) [..]`
pub fn show_caption(c: &Caption) -> String {
    let short = match &c.short {
        None => "Nothing".to_string(),
        Some(inlines) => format!("(Just {})", show_inlines(inlines)),
    };
    let long = match &c.long {
        None => "[]".to_string(),
        Some(blocks) => show_blocks(blocks),
    };
    format!("Caption {short} {long}")
}

pub fn show_row(r: &Row) -> String {
    let cells: Vec<String> = r.cells.iter().map(show_cell).collect();
    format!("Row {} [{}]", show_attr(&r.attr), cells.join(","))
}

pub fn show_cell(c: &Cell) -> String {
    format!(
        "Cell {} {} (RowSpan {}) (ColSpan {}) {}",
        show_attr(&c.attr),
        show_alignment(&c.alignment),
        c.row_span,
        c.col_span,
        show_blocks(&c.content)
    )
}

fn show_rows(rows: &[Row]) -> String {
    let shown: Vec<String> = rows.iter().map(show_row).collect();
    format!("[{}]", shown.join(","))
}

pub fn show_table_head(h: &TableHead) -> String {
    format!("TableHead {} {}", show_attr(&h.attr), show_rows(&h.rows))
}

pub fn show_table_foot(f: &TableFoot) -> String {
    format!("TableFoot {} {}", show_attr(&f.attr), show_rows(&f.rows))
}

pub fn show_table_body(b: &TableBody) -> String {
    format!(
        "TableBody {} (RowHeadColumns {}) {} {}",
        show_attr(&b.attr),
        b.rowhead_columns,
        show_rows(&b.head),
        show_rows(&b.body)
    )
}

/// `[Str "hello",Space,Str "there"]`
pub fn show_inlines(inlines: &[Inline]) -> String {
    let shown: Vec<String> = inlines.iter().map(show_inline).collect();
    format!("[{}]", shown.join(","))
}

/// `[Para [Str "p"]]`
pub fn show_blocks(blocks: &[Block]) -> String {
    let shown: Vec<String> = blocks.iter().map(show_block).collect();
    format!("[{}]", shown.join(","))
}

fn show_blocks_list(items: &[Vec<Block>]) -> String {
    let shown: Vec<String> = items.iter().map(|b| show_blocks(b)).collect();
    format!("[{}]", shown.join(","))
}

pub fn show_inline(inline: &Inline) -> String {
    match inline {
        Inline::Str(s) => format!("Str {}", show_string(&s.text)),
        Inline::Emph(e) => format!("Emph {}", show_inlines(&e.content)),
        Inline::Underline(e) => format!("Underline {}", show_inlines(&e.content)),
        Inline::Strong(e) => format!("Strong {}", show_inlines(&e.content)),
        Inline::Strikeout(e) => format!("Strikeout {}", show_inlines(&e.content)),
        Inline::Superscript(e) => format!("Superscript {}", show_inlines(&e.content)),
        Inline::Subscript(e) => format!("Subscript {}", show_inlines(&e.content)),
        Inline::SmallCaps(e) => format!("SmallCaps {}", show_inlines(&e.content)),
        Inline::Quoted(q) => format!(
            "Quoted {} {}",
            show_quote_type(&q.quote_type),
            show_inlines(&q.content)
        ),
        Inline::Cite(c) => {
            let citations: Vec<String> = c.citations.iter().map(show_citation).collect();
            format!(
                "Cite [{}] {}",
                citations.join(","),
                show_inlines(&c.content)
            )
        }
        Inline::Code(c) => format!("Code {} {}", show_attr(&c.attr), show_string(&c.text)),
        Inline::Space(_) => "Space".to_string(),
        Inline::SoftBreak(_) => "SoftBreak".to_string(),
        Inline::LineBreak(_) => "LineBreak".to_string(),
        Inline::Math(m) => format!(
            "Math {} {}",
            show_math_type(&m.math_type),
            show_string(&m.text)
        ),
        Inline::RawInline(r) => format!(
            "RawInline (Format {}) {}",
            show_string(&r.format),
            show_string(&r.text)
        ),
        Inline::Link(l) => format!(
            "Link {} {} {}",
            show_attr(&l.attr),
            show_inlines(&l.content),
            show_target(&l.target)
        ),
        Inline::Image(i) => format!(
            "Image {} {} {}",
            show_attr(&i.attr),
            show_inlines(&i.content),
            show_target(&i.target)
        ),
        Inline::Note(n) => format!("Note {}", show_blocks(&n.content)),
        Inline::Span(s) => format!("Span {} {}", show_attr(&s.attr), show_inlines(&s.content)),
        // q2 extensions below — no Pandoc counterpart; stable,
        // value-deterministic spellings in the same style.
        other => show_q2_inline_extension(other),
    }
}

fn show_q2_inline_extension(inline: &Inline) -> String {
    match inline {
        Inline::Insert(e) => format!("Insert {} {}", show_attr(&e.attr), show_inlines(&e.content)),
        Inline::Delete(e) => format!("Delete {} {}", show_attr(&e.attr), show_inlines(&e.content)),
        Inline::Highlight(e) => format!(
            "Highlight {} {}",
            show_attr(&e.attr),
            show_inlines(&e.content)
        ),
        Inline::EditComment(e) => format!(
            "EditComment {} {}",
            show_attr(&e.attr),
            show_inlines(&e.content)
        ),
        Inline::NoteReference(n) => format!("NoteReference {}", show_string(&n.id)),
        Inline::Shortcode(_) => "Shortcode".to_string(),
        Inline::Attr(_) => "InlineAttr".to_string(),
        Inline::Custom(_) => "Custom".to_string(),
        // Exhaustiveness: the Pandoc-standard variants are handled by
        // the caller; anything new lands here visibly.
        other => format!("<unshown inline: {other:?}>"),
    }
}

pub fn show_block(block: &Block) -> String {
    match block {
        Block::Plain(b) => format!("Plain {}", show_inlines(&b.content)),
        Block::Paragraph(b) => format!("Para {}", show_inlines(&b.content)),
        Block::LineBlock(b) => {
            let lines: Vec<String> = b.content.iter().map(|l| show_inlines(l)).collect();
            format!("LineBlock [{}]", lines.join(","))
        }
        Block::CodeBlock(b) => format!("CodeBlock {} {}", show_attr(&b.attr), show_string(&b.text)),
        Block::RawBlock(b) => format!(
            "RawBlock (Format {}) {}",
            show_string(&b.format),
            show_string(&b.text)
        ),
        Block::BlockQuote(b) => format!("BlockQuote {}", show_blocks(&b.content)),
        Block::OrderedList(b) => format!(
            "OrderedList {} {}",
            show_list_attributes(&b.attr),
            show_blocks_list(&b.content)
        ),
        Block::BulletList(b) => format!("BulletList {}", show_blocks_list(&b.content)),
        Block::DefinitionList(b) => {
            let items: Vec<String> = b
                .content
                .iter()
                .map(|(term, defs)| format!("({},{})", show_inlines(term), show_blocks_list(defs)))
                .collect();
            format!("DefinitionList [{}]", items.join(","))
        }
        Block::Header(b) => format!(
            "Header {} {} {}",
            b.level,
            show_attr(&b.attr),
            show_inlines(&b.content)
        ),
        Block::HorizontalRule(_) => "HorizontalRule".to_string(),
        Block::Table(t) => {
            let colspecs: Vec<String> = t
                .colspec
                .iter()
                .map(|(a, w)| format!("({},{})", show_alignment(a), show_col_width(w)))
                .collect();
            let bodies: Vec<String> = t.bodies.iter().map(show_table_body).collect();
            format!(
                "Table {} ({}) [{}] ({}) [{}] ({})",
                show_attr(&t.attr),
                show_caption(&t.caption),
                colspecs.join(","),
                show_table_head(&t.head),
                bodies.join(","),
                show_table_foot(&t.foot)
            )
        }
        Block::Figure(f) => format!(
            "Figure {} ({}) {}",
            show_attr(&f.attr),
            show_caption(&f.caption),
            show_blocks(&f.content)
        ),
        Block::Div(d) => format!("Div {} {}", show_attr(&d.attr), show_blocks(&d.content)),
        // q2 extensions — stable spellings, no Pandoc counterpart.
        Block::BlockMetadata(_) => "BlockMetadata".to_string(),
        Block::NoteDefinitionPara(b) => {
            format!("NoteDefinitionPara {}", show_inlines(&b.content))
        }
        Block::NoteDefinitionFencedBlock(b) => {
            format!("NoteDefinitionFencedBlock {}", show_blocks(&b.content))
        }
        Block::CaptionBlock(b) => format!("CaptionBlock {}", show_inlines(&b.content)),
        Block::Custom(_) => "Custom".to_string(),
    }
}

#[cfg(test)]
mod tests {
    // Expected strings in this module were captured from pandoc
    // 3.9.0.2 via `pandoc lua -e 'print(tostring(...))'`.
    use super::*;
    use hashlink::LinkedHashMap;

    #[test]
    fn show_string_escapes() {
        assert_eq!(show_string("hello"), r#""hello""#);
        assert_eq!(
            show_string("esc \"quote\" \\ back"),
            r#""esc \"quote\" \\ back""#
        );
        assert_eq!(show_string("a\nb\tc"), r#""a\nb\tc""#);
        // Haskell decimal escape for non-ASCII, with \& before a
        // following digit: show "é5" == "\"\\233\\&5\""
        assert_eq!(show_string("é5"), r#""\233\&5""#);
        assert_eq!(show_string("é"), r#""\233""#);
    }

    #[test]
    fn show_attr_matches_pandoc() {
        let mut kvs = LinkedHashMap::new();
        kvs.insert("k".to_string(), "v".to_string());
        let attr: Attr = ("i".to_string(), vec!["c".to_string()], kvs);
        assert_eq!(show_attr(&attr), r#"("i",["c"],[("k","v")])"#);
        let empty: Attr = (String::new(), vec![], LinkedHashMap::new());
        assert_eq!(show_attr(&empty), r#"("",[],[])"#);
    }

    #[test]
    fn show_inlines_matches_pandoc() {
        // Same coercion pandoc.Inlines("hello there") performs.
        let inlines = crate::lua::types::split_string_to_inlines("hello there");
        assert_eq!(show_inlines(&inlines), r#"[Str "hello",Space,Str "there"]"#);
        assert_eq!(show_inlines(&[]), "[]");
    }

    #[test]
    fn show_list_attributes_matches_pandoc() {
        assert_eq!(
            show_list_attributes(&(3, ListNumberStyle::Decimal, ListNumberDelim::Period)),
            "(3,Decimal,Period)"
        );
    }

    #[test]
    fn show_double_keeps_point() {
        assert_eq!(show_double(0.5), "0.5");
        assert_eq!(show_double(1.0), "1.0");
    }
}
