//! The bridge from q2's resolved terminal output style
//! ([`quarto_util::output_style`], decided once in `main`) to the
//! diagnostic renderer's [`TextRenderOptions`] (bd-be8ve52f, GH #1135).
//!
//! Every diagnostic q2 prints as text goes through one of these, so
//! `--color`, `NO_COLOR`, `CLICOLOR_FORCE` and "is stderr a terminal"
//! are honored the same way by every command.

use quarto_error_reporting::TextRenderOptions;
use quarto_util::output_style::{self, StreamStyle};

/// Render options that decorate exactly as `style` allows.
pub fn text_render_options(style: StreamStyle) -> TextRenderOptions {
    TextRenderOptions::default()
        .color(style.color)
        .hyperlinks(style.hyperlinks)
}

/// Render options for text printed to stderr in this process.
pub fn stderr_text_options() -> TextRenderOptions {
    text_render_options(output_style::get().stderr)
}

/// How `main` prints an error that ends the process, when it holds a
/// structured parse error ([`quarto_core::ParseError`], alone or as
/// [`quarto_core::QuartoError::Parse`]); `None` for any other error,
/// which `main` prints exactly as anyhow would (`Error: {err:?}`).
///
/// A parse error carries its own `Error: [Q-N-M]` header and source
/// snippet, so at the top of the chain it prints bare (like the render
/// abort paths). Under context it follows anyhow's layout: `Error:` and
/// the outermost message, then `Caused by:` with the remaining links,
/// the parse error rendered with `opts` in place of its plain
/// `Display`.
pub fn format_error_report(err: &anyhow::Error, opts: &TextRenderOptions) -> Option<String> {
    let links: Vec<&(dyn std::error::Error + 'static)> = err.chain().collect();
    let at = links.iter().position(|e| as_parse_error(*e).is_some())?;
    let rendered = as_parse_error(links[at])?.render_with(opts);
    if at == 0 {
        return Some(rendered);
    }
    // anyhow's `{:?}` layout, stopping at the parse error: its own
    // `source()` would only repeat it.
    let causes = &links[1..=at];
    let mut out = format!("Error: {}\n\nCaused by:", links[0]);
    for (i, cause) in causes.iter().enumerate() {
        let text = if i + 1 == causes.len() {
            rendered.clone()
        } else {
            cause.to_string()
        };
        let prefix = if causes.len() > 1 {
            format!("{i:>5}: ")
        } else {
            "    ".to_string()
        };
        let indent = " ".repeat(prefix.len());
        for (n, line) in text.lines().enumerate() {
            out.push('\n');
            out.push_str(if n == 0 { &prefix } else { &indent });
            out.push_str(line);
        }
    }
    Some(out)
}

/// The structured parse error behind one link of an error chain.
fn as_parse_error<'a>(
    e: &'a (dyn std::error::Error + 'static),
) -> Option<&'a quarto_core::ParseError> {
    match e.downcast_ref::<quarto_core::QuartoError>() {
        Some(quarto_core::QuartoError::Parse(pe)) => Some(pe),
        _ => e.downcast_ref::<quarto_core::ParseError>(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_core::{ParseError, QuartoError};
    use quarto_error_reporting::DiagnosticMessageBuilder;
    use quarto_source_map::{SourceContext, SourceInfo};

    fn located_parse_error() -> ParseError {
        let mut ctx = SourceContext::new();
        let file = ctx.add_file("doc.qmd".to_string(), Some("hello world\n".to_string()));
        let diagnostic = DiagnosticMessageBuilder::error("Bad greeting")
            .with_location(SourceInfo::original(file, 0, 5))
            .build();
        ParseError::new(vec![diagnostic], ctx)
    }

    fn color() -> TextRenderOptions {
        TextRenderOptions::default().hyperlinks(false)
    }

    #[test]
    fn a_top_level_parse_error_prints_bare_and_styled() {
        let err = anyhow::Error::from(QuartoError::Parse(located_parse_error()));
        let report = format_error_report(&err, &color()).expect("a parse error");
        assert!(report.contains("Bad greeting"), "{report}");
        assert!(report.contains("\u{1b}["), "{report:?}");
        assert!(!report.starts_with("Error: Error"), "{report}");
        assert!(!report.contains("Caused by"), "{report}");

        let plain = format_error_report(&err, &TextRenderOptions::plain()).unwrap();
        assert!(!plain.contains('\u{1b}'), "{plain:?}");
    }

    #[test]
    fn a_bare_parse_error_is_found_too() {
        let err = anyhow::Error::from(located_parse_error());
        let report = format_error_report(&err, &color()).expect("a parse error");
        assert!(report.contains("\u{1b}["), "{report:?}");
    }

    #[test]
    fn context_above_a_parse_error_keeps_anyhow_layout() {
        let err = anyhow::Error::from(QuartoError::Parse(located_parse_error()))
            .context("Failed to load the project");
        let report = format_error_report(&err, &color()).expect("a parse error");
        assert!(
            report.starts_with("Error: Failed to load the project\n"),
            "{report}"
        );
        assert!(report.contains("Caused by:"), "{report}");
        assert!(report.contains("\u{1b}["), "{report:?}");
        // The diagnostic appears once, not again as its own `source`
        // (one snippet header).
        assert_eq!(report.matches("doc.qmd:1:1").count(), 1, "{report}");
    }

    #[test]
    fn other_errors_are_left_to_anyhow() {
        let err = anyhow::anyhow!("plain failure").context("outer");
        assert_eq!(format_error_report(&err, &color()), None);
        let err = anyhow::Error::from(QuartoError::other("not a parse error"));
        assert_eq!(format_error_report(&err, &color()), None);
    }

    #[test]
    fn options_follow_each_half_of_the_style() {
        for (color, hyperlinks) in [(false, false), (true, false), (false, true), (true, true)] {
            let opts = text_render_options(StreamStyle { color, hyperlinks });
            assert_eq!(
                (opts.enable_color, opts.enable_hyperlinks),
                (color, hyperlinks)
            );
        }
    }
}
