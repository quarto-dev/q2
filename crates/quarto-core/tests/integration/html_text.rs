//! Shared helper: the reader-visible text of rendered HTML.

/// The rendered HTML's text with every tag removed (entities stay encoded).
///
/// Code blocks are syntax-highlighted, including `markdown` blocks (which
/// resolve to the built-in qmd highlighter, GH #1081), so their text is split
/// across `<span>`s and a raw-HTML substring check cannot see it. Checks for
/// what the reader sees use this; checks for markup (classes, markers) keep
/// using the raw HTML.
pub fn display_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}
