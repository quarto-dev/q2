/*
 * ansi.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * ANSI terminal writer for Pandoc AST using crossterm for styling.
 *
 * Phase 1 implementation: Plain and Para blocks only.
 * Other blocks panic with helpful messages indicating they need implementation.
 */

use crate::pandoc::{Attr, Block, Inline, Pandoc};
use crossterm::style::{Color, Stylize};
use std::io::Write;

/// Configuration for ANSI writer
#[derive(Debug, Clone)]
pub struct AnsiConfig {
    /// Enable colors and styling
    pub colors: bool,
    /// Terminal width for wrapping (0 = no wrapping)
    pub width: usize,
    /// Indent size for nested structures
    pub indent: usize,
}

impl Default for AnsiConfig {
    fn default() -> Self {
        Self {
            colors: true,
            width: 80,
            indent: 2,
        }
    }
}

/// Write Pandoc AST to ANSI terminal output
///
/// This writer uses crossterm to render styled text. Currently supports:
/// - Plain and Para blocks (with minimal inline rendering)
///
/// Other block types will panic with a message indicating they need implementation.
pub fn write<T: Write>(pandoc: &Pandoc, buf: &mut T) -> std::io::Result<()> {
    write_with_config(pandoc, buf, &AnsiConfig::default())
}

/// Write Pandoc AST with custom configuration
pub fn write_with_config<T: Write>(
    pandoc: &Pandoc,
    buf: &mut T,
    config: &AnsiConfig,
) -> std::io::Result<()> {
    for block in pandoc.blocks.iter() {
        write_block(block, buf, config)?;
    }
    Ok(())
}

fn write_block<T: Write>(block: &Block, buf: &mut T, config: &AnsiConfig) -> std::io::Result<()> {
    match block {
        Block::Plain(plain) => {
            write_inlines(&plain.content, buf, config)?;
        }
        Block::Paragraph(para) => {
            write_inlines(&para.content, buf, config)?;
            writeln!(buf)?;
        }

        // All other blocks panic with helpful messages
        Block::LineBlock(_) => {
            panic!(
                "LineBlock not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::CodeBlock(_) => {
            panic!(
                "CodeBlock not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::RawBlock(_) => {
            panic!(
                "RawBlock not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::BlockQuote(_) => {
            panic!(
                "BlockQuote not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::OrderedList(_) => {
            panic!(
                "OrderedList not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::BulletList(_) => {
            panic!(
                "BulletList not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::DefinitionList(_) => {
            panic!(
                "DefinitionList not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::Header(_) => {
            panic!(
                "Header not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::HorizontalRule(_) => {
            panic!(
                "HorizontalRule not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::Table(_) => {
            panic!(
                "Table not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::Figure(_) => {
            panic!(
                "Figure not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::Div(_) => {
            panic!(
                "Div not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::BlockMetadata(_) => {
            panic!(
                "BlockMetadata not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::NoteDefinitionPara(_) => {
            panic!(
                "NoteDefinitionPara not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::NoteDefinitionFencedBlock(_) => {
            panic!(
                "NoteDefinitionFencedBlock not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
        Block::CaptionBlock(_) => {
            panic!(
                "CaptionBlock not yet implemented in ANSI writer. Please add support in src/writers/ansi.rs"
            );
        }
    }
    Ok(())
}

fn write_inlines<T: Write>(
    inlines: &[Inline],
    buf: &mut T,
    config: &AnsiConfig,
) -> std::io::Result<()> {
    for inline in inlines {
        write_inline(inline, buf, config)?;
    }
    Ok(())
}

/// Parse a color attribute from span attributes
fn parse_color_attr(attr: &Attr, attr_name: &str) -> Option<Color> {
    let (_, _, attrs) = attr;

    for (key, value) in attrs {
        if key == attr_name {
            return parse_color_value(value);
        }
    }
    None
}

/// Parse a color value from various formats
fn parse_color_value(value: &str) -> Option<Color> {
    let value = value.trim();

    // Named basic colors (case-insensitive)
    match value.to_lowercase().as_str() {
        "black" => return Some(Color::Black),
        "dark-grey" | "darkgrey" | "dark-gray" | "darkgray" => return Some(Color::DarkGrey),
        "red" => return Some(Color::Red),
        "dark-red" | "darkred" => return Some(Color::DarkRed),
        "green" => return Some(Color::Green),
        "dark-green" | "darkgreen" => return Some(Color::DarkGreen),
        "yellow" => return Some(Color::Yellow),
        "dark-yellow" | "darkyellow" => return Some(Color::DarkYellow),
        "blue" => return Some(Color::Blue),
        "dark-blue" | "darkblue" => return Some(Color::DarkBlue),
        "magenta" => return Some(Color::Magenta),
        "dark-magenta" | "darkmagenta" => return Some(Color::DarkMagenta),
        "cyan" => return Some(Color::Cyan),
        "dark-cyan" | "darkcyan" => return Some(Color::DarkCyan),
        "white" => return Some(Color::White),
        "grey" | "gray" => return Some(Color::Grey),
        "reset" => return Some(Color::Reset),
        _ => {}
    }

    // Hex colors: #RRGGBB or #RGB
    if value.starts_with('#') {
        if value.len() == 7 {
            // #RRGGBB format
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&value[1..3], 16),
                u8::from_str_radix(&value[3..5], 16),
                u8::from_str_radix(&value[5..7], 16),
            ) {
                return Some(Color::Rgb { r, g, b });
            }
        } else if value.len() == 4 {
            // #RGB format - expand to #RRGGBB
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&value[1..2], 16),
                u8::from_str_radix(&value[2..3], 16),
                u8::from_str_radix(&value[3..4], 16),
            ) {
                return Some(Color::Rgb {
                    r: r * 17, // 0xF -> 0xFF
                    g: g * 17,
                    b: b * 17,
                });
            }
        }
    }

    // RGB function: rgb(255, 128, 0) or rgb(255,128,0)
    if value.starts_with("rgb(") && value.ends_with(')') {
        let inner = &value[4..value.len() - 1];
        let parts: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
        if parts.len() == 3 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                parts[0].parse::<u8>(),
                parts[1].parse::<u8>(),
                parts[2].parse::<u8>(),
            ) {
                return Some(Color::Rgb { r, g, b });
            }
        }
    }

    // ANSI palette: ansi(42) or ansi-42
    if value.starts_with("ansi(") && value.ends_with(')') {
        let num_str = &value[5..value.len() - 1];
        if let Ok(ansi_value) = num_str.parse::<u8>() {
            return Some(Color::AnsiValue(ansi_value));
        }
    } else if value.starts_with("ansi-") {
        let num_str = &value[5..];
        if let Ok(ansi_value) = num_str.parse::<u8>() {
            return Some(Color::AnsiValue(ansi_value));
        }
    }

    None
}

fn write_inline<T: Write>(
    inline: &Inline,
    buf: &mut T,
    config: &AnsiConfig,
) -> std::io::Result<()> {
    match inline {
        // Basic text elements
        Inline::Str(s) => {
            write!(buf, "{}", s.text)?;
        }
        Inline::Space(_) => {
            write!(buf, " ")?;
        }
        Inline::SoftBreak(_) => {
            write!(buf, "\n")?;
        }
        Inline::LineBreak(_) => {
            write!(buf, "\n")?;
        }

        // Styled text
        Inline::Emph(emph) => {
            if config.colors {
                write!(buf, "{}", format_inlines(&emph.content, config).italic())?;
            } else {
                write_inlines(&emph.content, buf, config)?;
            }
        }
        Inline::Underline(underline) => {
            if config.colors {
                write!(
                    buf,
                    "{}",
                    format_inlines(&underline.content, config).underlined()
                )?;
            } else {
                write_inlines(&underline.content, buf, config)?;
            }
        }
        Inline::Strong(strong) => {
            if config.colors {
                write!(buf, "{}", format_inlines(&strong.content, config).bold())?;
            } else {
                write_inlines(&strong.content, buf, config)?;
            }
        }
        Inline::Strikeout(strikeout) => {
            if config.colors {
                write!(
                    buf,
                    "{}",
                    format_inlines(&strikeout.content, config).crossed_out()
                )?;
            } else {
                write_inlines(&strikeout.content, buf, config)?;
            }
        }
        Inline::Superscript(superscript) => {
            // No direct superscript support in terminal, just render content
            write!(buf, "^")?;
            write_inlines(&superscript.content, buf, config)?;
        }
        Inline::Subscript(subscript) => {
            // No direct subscript support in terminal, just render content
            write!(buf, "_")?;
            write_inlines(&subscript.content, buf, config)?;
        }
        Inline::SmallCaps(smallcaps) => {
            // No small caps in terminal, just render as-is
            write_inlines(&smallcaps.content, buf, config)?;
        }
        Inline::Quoted(quoted) => {
            use crate::pandoc::QuoteType;
            let (open, close) = match quoted.quote_type {
                QuoteType::SingleQuote => ("'", "'"),
                QuoteType::DoubleQuote => ("\"", "\""),
            };
            write!(buf, "{}", open)?;
            write_inlines(&quoted.content, buf, config)?;
            write!(buf, "{}", close)?;
        }
        Inline::Cite(cite) => {
            // Render citations as plain text for now
            write_inlines(&cite.content, buf, config)?;
        }
        Inline::Code(code) => {
            if config.colors {
                write!(buf, "{}", code.text.as_str().on_dark_grey().white())?;
            } else {
                write!(buf, "`{}`", code.text)?;
            }
        }
        Inline::Math(math) => {
            if config.colors {
                write!(buf, "{}", math.text.as_str().yellow())?;
            } else {
                write!(buf, "${}", math.text)?;
            }
        }
        Inline::RawInline(raw) => {
            // Pass through raw content as-is
            write!(buf, "{}", raw.text)?;
        }
        Inline::Link(link) => {
            if config.colors {
                write!(
                    buf,
                    "{}",
                    format_inlines(&link.content, config).cyan().underlined()
                )?;
            } else {
                write_inlines(&link.content, buf, config)?;
                write!(buf, " ({}, {})", link.target.0, link.target.1)?;
            }
        }
        Inline::Image(image) => {
            // Render images as [Image: alt text]
            write!(buf, "[Image: ")?;
            write_inlines(&image.content, buf, config)?;
            write!(buf, "]")?;
        }
        Inline::Note(note) => {
            // Render footnotes as superscript marker for now
            write!(buf, "^[{}]", note.content.len())?;
        }
        Inline::Span(span) => {
            // Check for color attributes
            let fg_color = parse_color_attr(&span.attr, "color");
            let bg_color = parse_color_attr(&span.attr, "background-color");

            // Apply colors if enabled and present
            if config.colors && (fg_color.is_some() || bg_color.is_some()) {
                let content_str = format_inlines(&span.content, config);

                let styled = match (fg_color, bg_color) {
                    (Some(fg), Some(bg)) => content_str.with(fg).on(bg),
                    (Some(fg), None) => content_str.with(fg),
                    (None, Some(bg)) => content_str.on(bg),
                    (None, None) => unreachable!(), // We checked above
                };

                write!(buf, "{}", styled)?;
            } else {
                // No color support or no color attrs, just render content
                write_inlines(&span.content, buf, config)?;
            }
        }

        // Quarto extensions - minimal handling for now
        Inline::Shortcode(_) => {
            // Ignore shortcodes in ANSI output
        }
        Inline::NoteReference(_) => {
            // Ignore note references
        }
        Inline::Attr(_, _) => {
            // Ignore standalone attributes
        }
        Inline::Insert(insert) => {
            // Render inserts as plain text
            write_inlines(&insert.content, buf, config)?;
        }
        Inline::Delete(delete) => {
            // Render deletes as strikethrough if colors enabled
            if config.colors {
                write!(
                    buf,
                    "{}",
                    format_inlines(&delete.content, config).crossed_out()
                )?;
            } else {
                write_inlines(&delete.content, buf, config)?;
            }
        }
        Inline::Highlight(highlight) => {
            // Render highlights with background color if enabled
            if config.colors {
                write!(
                    buf,
                    "{}",
                    format_inlines(&highlight.content, config)
                        .on_yellow()
                        .black()
                )?;
            } else {
                write_inlines(&highlight.content, buf, config)?;
            }
        }
        Inline::EditComment(_) => {
            // Ignore edit comments in output
        }
    }
    Ok(())
}

/// Helper to format inlines to a string for styling
fn format_inlines(inlines: &[Inline], config: &AnsiConfig) -> String {
    let mut buf = Vec::new();
    write_inlines(inlines, &mut buf, config).unwrap();
    String::from_utf8(buf).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = AnsiConfig::default();
        assert_eq!(config.colors, true);
        assert_eq!(config.width, 80);
        assert_eq!(config.indent, 2);
    }

    #[test]
    fn test_empty_document() {
        let pandoc = Pandoc {
            meta: Default::default(),
            blocks: vec![],
        };

        let mut buf = Vec::new();
        write(&pandoc, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert_eq!(output, "");
    }

    // Color parsing tests
    #[test]
    fn test_parse_basic_colors() {
        assert!(matches!(parse_color_value("red"), Some(Color::Red)));
        assert!(matches!(parse_color_value("blue"), Some(Color::Blue)));
        assert!(matches!(parse_color_value("green"), Some(Color::Green)));
        assert!(matches!(parse_color_value("yellow"), Some(Color::Yellow)));
        assert!(matches!(parse_color_value("cyan"), Some(Color::Cyan)));
        assert!(matches!(parse_color_value("magenta"), Some(Color::Magenta)));
        assert!(matches!(parse_color_value("white"), Some(Color::White)));
        assert!(matches!(parse_color_value("black"), Some(Color::Black)));
    }

    #[test]
    fn test_parse_dark_colors() {
        assert!(matches!(
            parse_color_value("dark-red"),
            Some(Color::DarkRed)
        ));
        assert!(matches!(parse_color_value("darkred"), Some(Color::DarkRed)));
        assert!(matches!(
            parse_color_value("dark-blue"),
            Some(Color::DarkBlue)
        ));
        assert!(matches!(
            parse_color_value("dark-grey"),
            Some(Color::DarkGrey)
        ));
        assert!(matches!(
            parse_color_value("darkgray"),
            Some(Color::DarkGrey)
        ));
    }

    #[test]
    fn test_parse_hex_colors() {
        // Full hex format #RRGGBB
        assert!(matches!(
            parse_color_value("#FF0000"),
            Some(Color::Rgb { r: 255, g: 0, b: 0 })
        ));
        assert!(matches!(
            parse_color_value("#00FF00"),
            Some(Color::Rgb { r: 0, g: 255, b: 0 })
        ));
        assert!(matches!(
            parse_color_value("#0000FF"),
            Some(Color::Rgb { r: 0, g: 0, b: 255 })
        ));

        // Short hex format #RGB
        assert!(matches!(
            parse_color_value("#F00"),
            Some(Color::Rgb { r: 255, g: 0, b: 0 })
        ));
        assert!(matches!(
            parse_color_value("#0F0"),
            Some(Color::Rgb { r: 0, g: 255, b: 0 })
        ));
        assert!(matches!(
            parse_color_value("#00F"),
            Some(Color::Rgb { r: 0, g: 0, b: 255 })
        ));
    }

    #[test]
    fn test_parse_rgb_function() {
        assert!(matches!(
            parse_color_value("rgb(255, 128, 0)"),
            Some(Color::Rgb {
                r: 255,
                g: 128,
                b: 0
            })
        ));
        assert!(matches!(
            parse_color_value("rgb(0,0,0)"),
            Some(Color::Rgb { r: 0, g: 0, b: 0 })
        ));
        assert!(matches!(
            parse_color_value("rgb(255, 255, 255)"),
            Some(Color::Rgb {
                r: 255,
                g: 255,
                b: 255
            })
        ));
    }

    #[test]
    fn test_parse_ansi_colors() {
        assert!(matches!(
            parse_color_value("ansi(42)"),
            Some(Color::AnsiValue(42))
        ));
        assert!(matches!(
            parse_color_value("ansi-196"),
            Some(Color::AnsiValue(196))
        ));
        assert!(matches!(
            parse_color_value("ansi(0)"),
            Some(Color::AnsiValue(0))
        ));
        assert!(matches!(
            parse_color_value("ansi(255)"),
            Some(Color::AnsiValue(255))
        ));
    }

    #[test]
    fn test_parse_invalid_colors() {
        assert!(parse_color_value("notacolor").is_none());
        assert!(parse_color_value("#ZZZ").is_none());
        assert!(parse_color_value("rgb(999, 0, 0)").is_none());
        assert!(parse_color_value("ansi(999)").is_none());
        assert!(parse_color_value("").is_none());
    }

    #[test]
    fn test_parse_color_case_insensitive() {
        assert!(matches!(parse_color_value("RED"), Some(Color::Red)));
        assert!(matches!(parse_color_value("Blue"), Some(Color::Blue)));
        assert!(matches!(
            parse_color_value("DARK-RED"),
            Some(Color::DarkRed)
        ));
    }

    // Note: More comprehensive tests should be integration tests that parse
    // actual markdown and verify the ANSI output, rather than trying to
    // manually construct the complex Pandoc AST structures.
}
