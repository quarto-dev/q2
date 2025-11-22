/*
 * ansi.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * ANSI terminal writer for Pandoc AST using crossterm for styling.
 *
 * Phase 1 implementation: Plain and Para blocks only.
 * Other blocks panic with helpful messages indicating they need implementation.
 */

use crate::pandoc::{
    Attr, Block, BlockQuote, BulletList, DefinitionList, Div, Inline, OrderedList, Pandoc,
};
use crossterm::style::{Color, Stylize};
use std::io::Write;

#[cfg(feature = "terminal-hyperlinks")]
use supports_hyperlinks;

/// Tracks the spacing behavior of the last block written
#[derive(Clone, Copy, PartialEq, Debug)]
enum LastBlockSpacing {
    None,      // Nothing written yet
    Plain,     // Plain block (ends with single \n)
    Paragraph, // Para/Div/etc (ends with blank line)
}

/// Tracks the current style context for nested styled elements
/// This allows us to restore parent colors after child styled elements complete
#[derive(Debug, Clone)]
struct StyleContext {
    fg_stack: Vec<Option<Color>>,
    bg_stack: Vec<Option<Color>>,
}

impl StyleContext {
    fn new() -> Self {
        Self {
            fg_stack: vec![None], // Start with default (no color)
            bg_stack: vec![None], // Start with default (no color)
        }
    }

    fn push_fg(&mut self, color: Option<Color>) {
        self.fg_stack.push(color);
    }

    fn pop_fg(&mut self) {
        if self.fg_stack.len() > 1 {
            self.fg_stack.pop();
        }
    }

    fn current_fg(&self) -> Option<Color> {
        self.fg_stack.last().copied().flatten()
    }

    fn push_bg(&mut self, color: Option<Color>) {
        self.bg_stack.push(color);
    }

    fn pop_bg(&mut self) {
        if self.bg_stack.len() > 1 {
            self.bg_stack.pop();
        }
    }

    fn current_bg(&self) -> Option<Color> {
        self.bg_stack.last().copied().flatten()
    }

    /// Write ANSI codes to restore the current style context
    fn restore_current_style<W: Write + ?Sized>(&self, buf: &mut W) -> std::io::Result<()> {
        // Restore background first, then foreground (order matters for some terminals)
        if let Some(bg) = self.current_bg() {
            write!(buf, "\x1b[{}m", color_to_ansi_bg(bg))?;
        } else {
            write!(buf, "\x1b[49m")?; // Reset to default background
        }

        if let Some(fg) = self.current_fg() {
            write!(buf, "\x1b[{}m", color_to_ansi_fg(fg))?;
        } else {
            write!(buf, "\x1b[39m")?; // Reset to default foreground
        }

        Ok(())
    }
}

/// Context for ANSI writer containing error accumulation.
///
/// This struct accumulates diagnostic errors during AST traversal,
/// allowing the writer to report unsupported features rather than panicking.
struct AnsiWriterContext {
    errors: Vec<quarto_error_reporting::DiagnosticMessage>,
}

impl AnsiWriterContext {
    fn new() -> Self {
        AnsiWriterContext { errors: Vec::new() }
    }
}

/// Convert a Color to ANSI foreground code
fn color_to_ansi_fg(color: Color) -> String {
    match color {
        Color::Black => "30".to_string(),
        Color::DarkGrey => "90".to_string(),
        Color::Red => "91".to_string(),
        Color::DarkRed => "31".to_string(),
        Color::Green => "92".to_string(),
        Color::DarkGreen => "32".to_string(),
        Color::Yellow => "93".to_string(),
        Color::DarkYellow => "33".to_string(),
        Color::Blue => "94".to_string(),
        Color::DarkBlue => "34".to_string(),
        Color::Magenta => "95".to_string(),
        Color::DarkMagenta => "35".to_string(),
        Color::Cyan => "96".to_string(),
        Color::DarkCyan => "36".to_string(),
        Color::White => "97".to_string(),
        Color::Grey => "37".to_string(),
        Color::Rgb { r, g, b } => format!("38;2;{};{};{}", r, g, b),
        Color::AnsiValue(v) => format!("38;5;{}", v),
        Color::Reset => "39".to_string(),
    }
}

/// Convert a Color to ANSI background code
fn color_to_ansi_bg(color: Color) -> String {
    match color {
        Color::Black => "40".to_string(),
        Color::DarkGrey => "100".to_string(),
        Color::Red => "101".to_string(),
        Color::DarkRed => "41".to_string(),
        Color::Green => "102".to_string(),
        Color::DarkGreen => "42".to_string(),
        Color::Yellow => "103".to_string(),
        Color::DarkYellow => "43".to_string(),
        Color::Blue => "104".to_string(),
        Color::DarkBlue => "44".to_string(),
        Color::Magenta => "105".to_string(),
        Color::DarkMagenta => "45".to_string(),
        Color::Cyan => "106".to_string(),
        Color::DarkCyan => "46".to_string(),
        Color::White => "107".to_string(),
        Color::Grey => "47".to_string(),
        Color::Rgb { r, g, b } => format!("48;2;{};{};{}", r, g, b),
        Color::AnsiValue(v) => format!("48;5;{}", v),
        Color::Reset => "49".to_string(),
    }
}

/// Configuration for ANSI writer
#[derive(Debug, Clone)]
pub struct AnsiConfig {
    /// Enable colors and styling
    pub colors: bool,
    /// Terminal width for wrapping (0 = no wrapping)
    pub width: usize,
    /// Indent size for nested structures
    pub indent: usize,
    /// Enable clickable hyperlinks (OSC 8)
    pub hyperlinks: bool,
}

impl Default for AnsiConfig {
    fn default() -> Self {
        Self {
            colors: true,
            width: Self::detect_terminal_width(),
            indent: 2,
            hyperlinks: Self::detect_hyperlink_support(),
        }
    }
}

impl AnsiConfig {
    /// Detect terminal width, defaulting to 80 if detection fails
    /// Can be overridden with QUARTO_TERMINAL_WIDTH environment variable
    fn detect_terminal_width() -> usize {
        // Check for environment variable override first
        if let Ok(width_str) = std::env::var("QUARTO_TERMINAL_WIDTH") {
            if let Ok(width) = width_str.parse::<usize>() {
                return width;
            }
        }

        // Otherwise detect from terminal
        use crossterm::terminal::size;
        size().ok().map(|(cols, _rows)| cols as usize).unwrap_or(80)
    }

    /// Detect if the terminal supports hyperlinks
    fn detect_hyperlink_support() -> bool {
        #[cfg(feature = "terminal-hyperlinks")]
        {
            supports_hyperlinks::on(supports_hyperlinks::Stream::Stdout)
        }
        #[cfg(not(feature = "terminal-hyperlinks"))]
        {
            false
        }
    }
}

/// Context for writing bullet list items with proper indentation and markers
struct BulletListContext<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    at_line_start: bool,
    is_first_line: bool,
    bullet: &'static str,
    config: &'a AnsiConfig,
}

impl<'a, W: Write + ?Sized> BulletListContext<'a, W> {
    fn new(inner: &'a mut W, depth: usize, config: &'a AnsiConfig) -> Self {
        let bullet = match depth % 3 {
            0 => "*  ",
            1 => "-  ",
            2 => "+  ",
            _ => unreachable!(),
        };
        Self {
            inner,
            at_line_start: true,
            is_first_line: true,
            bullet,
            config,
        }
    }
}

impl<'a, W: Write + ?Sized> Write for BulletListContext<'a, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut written = 0;
        for &byte in buf {
            if self.at_line_start {
                if self.is_first_line {
                    // Write bullet marker in muted color (dark grey)
                    if self.config.colors {
                        write!(self.inner, "{}", self.bullet.to_string().dark_grey())?;
                    } else {
                        self.inner.write_all(self.bullet.as_bytes())?;
                    }
                    self.is_first_line = false;
                } else {
                    self.inner.write_all(b"   ")?; // 3 spaces for continuation
                }
                self.at_line_start = false;
            }
            self.inner.write_all(&[byte])?;
            written += 1;
            if byte == b'\n' {
                self.at_line_start = true;
            }
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Context for writing ordered list items with calculated indentation
struct OrderedListContext<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    at_line_start: bool,
    is_first_line: bool,
    item_num_str: String,
    continuation_indent: String,
    config: &'a AnsiConfig,
}

impl<'a, W: Write + ?Sized> OrderedListContext<'a, W> {
    fn new(inner: &'a mut W, item_num: usize, indent_width: usize, config: &'a AnsiConfig) -> Self {
        let item_num_str = format!("{}. ", item_num);
        let continuation_indent = " ".repeat(indent_width.max(item_num_str.len()));
        Self {
            inner,
            at_line_start: true,
            is_first_line: true,
            item_num_str,
            continuation_indent,
            config,
        }
    }
}

impl<'a, W: Write + ?Sized> Write for OrderedListContext<'a, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut written = 0;
        for &byte in buf {
            if self.at_line_start {
                if self.is_first_line {
                    // Write number marker in muted color (dark grey)
                    if self.config.colors {
                        write!(self.inner, "{}", self.item_num_str.as_str().dark_grey())?;
                    } else {
                        self.inner.write_all(self.item_num_str.as_bytes())?;
                    }
                    self.is_first_line = false;
                } else {
                    self.inner.write_all(self.continuation_indent.as_bytes())?;
                }
                self.at_line_start = false;
            }
            self.inner.write_all(&[byte])?;
            written += 1;
            if byte == b'\n' {
                self.at_line_start = true;
            }
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Context for writing block quotes with vertical line marker
struct BlockQuoteContext<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    at_line_start: bool,
    config: &'a AnsiConfig,
}

impl<'a, W: Write + ?Sized> BlockQuoteContext<'a, W> {
    fn new(inner: &'a mut W, config: &'a AnsiConfig) -> Self {
        Self {
            inner,
            at_line_start: true,
            config,
        }
    }
}

impl<'a, W: Write + ?Sized> Write for BlockQuoteContext<'a, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut written = 0;
        for &byte in buf {
            if self.at_line_start {
                // Write vertical line marker in muted color (dark grey)
                if self.config.colors {
                    write!(self.inner, "{}", "│ ".dark_grey())?;
                } else {
                    self.inner.write_all(b"> ")?; // Fallback to > for no-color
                }
                self.at_line_start = false;
            }
            self.inner.write_all(&[byte])?;
            written += 1;
            if byte == b'\n' {
                self.at_line_start = true;
            }
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Context for writing divs with line-by-line color styling
struct DivContext<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    fg_color: Option<Color>,
    bg_color: Option<Color>,
    line_buffer: Vec<u8>,
    config: &'a AnsiConfig,
}

impl<'a, W: Write + ?Sized> DivContext<'a, W> {
    fn new(inner: &'a mut W, fg: Option<Color>, bg: Option<Color>, config: &'a AnsiConfig) -> Self {
        Self {
            inner,
            fg_color: fg,
            bg_color: bg,
            line_buffer: Vec::new(),
            config,
        }
    }

    fn flush_line(&mut self) -> std::io::Result<()> {
        if self.line_buffer.is_empty() {
            return Ok(());
        }

        if self.config.colors && (self.fg_color.is_some() || self.bg_color.is_some()) {
            let line = String::from_utf8(self.line_buffer.clone())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

            let styled = match (self.fg_color, self.bg_color) {
                (Some(fg), Some(bg)) => line.with(fg).on(bg),
                (Some(fg), None) => line.with(fg),
                (None, Some(bg)) => line.on(bg),
                (None, None) => unreachable!(),
            };

            write!(self.inner, "{}", styled)?;
        } else {
            self.inner.write_all(&self.line_buffer)?;
        }

        self.line_buffer.clear();
        Ok(())
    }
}

impl<'a, W: Write + ?Sized> Write for DivContext<'a, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut written = 0;
        for &byte in buf {
            if byte == b'\n' {
                self.flush_line()?;
                self.inner.write_all(&[b'\n'])?;
            } else {
                self.line_buffer.push(byte);
            }
            written += 1;
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flush_line()?;
        self.inner.flush()
    }
}

impl<'a, W: Write + ?Sized> Drop for DivContext<'a, W> {
    fn drop(&mut self) {
        let _ = self.flush_line();
    }
}

/// Calculate indent width for ordered lists based on maximum item number
fn calculate_indent_width(start: usize, count: usize) -> usize {
    if count == 0 {
        return 3; // Minimum "1. "
    }
    let max_num = start + count - 1;
    let max_digits = if max_num == 0 {
        1
    } else {
        (max_num as f64).log10().floor() as usize + 1
    };
    max_digits + 2 // digits + ". "
}

/// Write Pandoc AST to ANSI terminal output
///
/// This writer uses crossterm to render styled text. Currently supports:
/// - Plain and Para blocks (with minimal inline rendering)
///
/// Other block types will emit proper diagnostic errors.
pub fn write<T: Write>(
    pandoc: &Pandoc,
    buf: &mut T,
) -> Result<(), Vec<quarto_error_reporting::DiagnosticMessage>> {
    write_with_config(pandoc, buf, &AnsiConfig::default())
}

/// Write Pandoc AST with custom configuration
pub fn write_with_config<T: Write>(
    pandoc: &Pandoc,
    buf: &mut T,
    config: &AnsiConfig,
) -> Result<(), Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut ctx = AnsiWriterContext::new();

    // Try to write - IO errors are fatal
    if let Err(e) = write_impl(pandoc, buf, config, &mut ctx) {
        // IO error - wrap and return
        return Err(vec![
            quarto_error_reporting::DiagnosticMessageBuilder::error("IO error during write")
                .with_code("Q-3-1")
                .problem(format!("Failed to write ANSI output: {}", e))
                .build(),
        ]);
    }

    // Check for accumulated feature errors
    if !ctx.errors.is_empty() {
        return Err(ctx.errors);
    }

    Ok(())
}

fn write_impl<T: Write>(
    pandoc: &Pandoc,
    buf: &mut T,
    config: &AnsiConfig,
    ctx: &mut AnsiWriterContext,
) -> std::io::Result<()> {
    let mut last_spacing = LastBlockSpacing::None;

    for block in pandoc.blocks.iter() {
        // Determine if we need a blank line before this block
        let needs_blank = match (&last_spacing, block) {
            (LastBlockSpacing::Plain, Block::Plain(_)) => false, // Consecutive Plains: single \n
            (LastBlockSpacing::None, _) => false,                // First block
            _ => true,                                           // All other cases: blank line
        };

        if needs_blank {
            writeln!(buf)?; // Extra \n for blank line
        }

        last_spacing = write_block_with_depth(block, buf, config, 0, 0, ctx)?;
    }
    Ok(())
}

fn write_block_with_depth(
    block: &Block,
    buf: &mut dyn Write,
    config: &AnsiConfig,
    list_depth: usize,
    indent_chars: usize, // Total character indentation from all contexts
    ctx: &mut AnsiWriterContext,
) -> std::io::Result<LastBlockSpacing> {
    let mut style_ctx = StyleContext::new();
    match block {
        Block::Plain(plain) => {
            write_inlines(&plain.content, buf, config, &mut style_ctx)?;
            writeln!(buf)?;
            Ok(LastBlockSpacing::Plain)
        }
        Block::Paragraph(para) => {
            write_inlines(&para.content, buf, config, &mut style_ctx)?;
            writeln!(buf)?;
            Ok(LastBlockSpacing::Paragraph)
        }
        Block::Div(div) => write_div(div, buf, config, list_depth, indent_chars, ctx),
        Block::BulletList(list) => {
            write_bulletlist(list, buf, config, list_depth, indent_chars, ctx)
        }
        Block::OrderedList(list) => {
            write_orderedlist(list, buf, config, list_depth, indent_chars, ctx)
        }
        Block::DefinitionList(list) => {
            write_definitionlist(list, buf, config, list_depth, indent_chars, ctx)
        }
        Block::BlockQuote(bq) => write_blockquote(bq, buf, config, list_depth, indent_chars, ctx),

        // All other blocks emit diagnostic errors
        Block::LineBlock(_) => {
            ctx.errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "LineBlock not supported in ANSI format",
                )
                .with_code("Q-3-50")
                .problem("LineBlock elements cannot be rendered in ANSI terminal output")
                .add_detail("ANSI output format has limited block type support")
                .add_hint("Consider using a different output format for documents with line blocks")
                .build(),
            );
            Ok(LastBlockSpacing::None)
        }
        Block::CodeBlock(_) => {
            ctx.errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "CodeBlock not supported in ANSI format",
                )
                .with_code("Q-3-51")
                .problem("CodeBlock elements cannot be rendered in ANSI terminal output")
                .add_detail("ANSI output format does not yet support code blocks")
                .add_hint("Consider using a different output format for documents with code blocks")
                .build(),
            );
            Ok(LastBlockSpacing::None)
        }
        Block::RawBlock(raw) => {
            // Only render raw content if format matches "ansi"
            if raw.format == "ansi" {
                // Write content directly with single newline (Plain semantics)
                write!(buf, "{}", raw.text)?;
                writeln!(buf)?;
                Ok(LastBlockSpacing::Plain)
            } else {
                // Skip blocks with wrong format - return None to indicate nothing written
                Ok(LastBlockSpacing::None)
            }
        }
        Block::Header(header) => {
            // Format the header content with styling
            let content = format_inlines(&header.content, config, &mut style_ctx);

            // Apply level-specific styling
            let styled_content = if config.colors {
                match header.level {
                    1 => {
                        // H1: bright (Color::White) and bold, centered
                        let text = content.white().bold().to_string();
                        let text_width = display_width(&text);
                        let available_width = if config.width > indent_chars {
                            config.width - indent_chars
                        } else {
                            config.width
                        };

                        // Calculate left padding for centering
                        let left_padding = if available_width > text_width {
                            (available_width - text_width) / 2
                        } else {
                            0
                        };

                        format!("{}{}", " ".repeat(left_padding), text)
                    }
                    2 => {
                        // H2: bright and bold
                        content.white().bold().to_string()
                    }
                    3 => {
                        // H3: bright
                        content.white().to_string()
                    }
                    4 => {
                        // H4: muted
                        content.dark_grey().to_string()
                    }
                    _ => {
                        // H5, H6: default
                        content
                    }
                }
            } else {
                content
            };

            writeln!(buf, "{}", styled_content)?;
            Ok(LastBlockSpacing::Paragraph)
        }
        Block::HorizontalRule(_) => {
            // Calculate effective width accounting for indentation
            let line_width = if config.width > indent_chars {
                config.width - indent_chars
            } else {
                10 // Minimum width if indentation is too large
            };

            let line = "─".repeat(line_width);
            if config.colors {
                writeln!(buf, "{}", line.dark_grey())?;
            } else {
                writeln!(buf, "{}", line)?;
            }
            Ok(LastBlockSpacing::Paragraph)
        }
        Block::Table(_) => {
            ctx.errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Table not supported in ANSI format",
                )
                .with_code("Q-3-52")
                .problem("Table elements cannot be rendered in ANSI terminal output")
                .add_detail("ANSI output format does not yet support tables")
                .add_hint("Consider using a different output format for documents with tables")
                .build(),
            );
            Ok(LastBlockSpacing::None)
        }
        Block::Figure(_) => {
            ctx.errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Figure not supported in ANSI format",
                )
                .with_code("Q-3-53")
                .problem("Figure elements cannot be rendered in ANSI terminal output")
                .add_detail("ANSI output format does not yet support figures")
                .add_hint("Consider using a different output format for documents with figures")
                .build(),
            );
            Ok(LastBlockSpacing::None)
        }
        Block::BlockMetadata(_) => {
            ctx.errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "BlockMetadata not supported in ANSI format",
                )
                .with_code("Q-3-20")
                .problem("BlockMetadata elements cannot be rendered in ANSI terminal output")
                .add_detail(
                    "Block-level metadata is a Quarto extension not representable in ANSI format",
                )
                .add_hint("Metadata blocks are typically processed before rendering")
                .build(),
            );
            Ok(LastBlockSpacing::None)
        }
        Block::NoteDefinitionPara(_) => {
            ctx.errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "NoteDefinitionPara not supported in ANSI format",
                )
                .with_code("Q-3-54")
                .problem("Note definitions cannot be rendered in ANSI terminal output")
                .add_detail("ANSI output format does not yet support footnotes")
                .add_hint("Consider using a different output format for documents with footnotes")
                .build(),
            );
            Ok(LastBlockSpacing::None)
        }
        Block::NoteDefinitionFencedBlock(_) => {
            ctx.errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "NoteDefinitionFencedBlock not supported in ANSI format",
                )
                .with_code("Q-3-55")
                .problem("Fenced note definitions cannot be rendered in ANSI terminal output")
                .add_detail("ANSI output format does not yet support footnotes")
                .add_hint("Consider using a different output format for documents with footnotes")
                .build(),
            );
            Ok(LastBlockSpacing::None)
        }
        Block::CaptionBlock(_) => {
            ctx.errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Caption block not supported",
                )
                .with_code("Q-3-21")
                .problem("Standalone caption blocks cannot be rendered in ANSI format")
                .add_detail(
                    "Caption blocks should be attached to figures or tables during postprocessing. \
                     This may indicate a postprocessing issue or filter-generated orphaned caption.",
                )
                .add_hint("Check for bugs in postprocessing or filters producing orphaned captions")
                .build(),
            );
            Ok(LastBlockSpacing::None)
        }
    }
}

/// Write a Div block with optional color styling
fn write_div(
    div: &Div,
    buf: &mut dyn Write,
    config: &AnsiConfig,
    list_depth: usize,
    indent_chars: usize,
    ansi_ctx: &mut AnsiWriterContext,
) -> std::io::Result<LastBlockSpacing> {
    let fg_color = parse_color_attr(&div.attr, "color");
    let bg_color = parse_color_attr(&div.attr, "background-color");

    // If colors are present, use DivContext for line-by-line styling
    if fg_color.is_some() || bg_color.is_some() {
        let mut div_ctx = DivContext::new(buf, fg_color, bg_color, config);
        for block in &div.content {
            write_block_with_depth(
                block,
                &mut div_ctx,
                config,
                list_depth,
                indent_chars,
                ansi_ctx,
            )?;
        }
        div_ctx.flush()?;
    } else {
        // No colors, write blocks directly
        for block in &div.content {
            write_block_with_depth(block, buf, config, list_depth, indent_chars, ansi_ctx)?;
        }
    }

    Ok(LastBlockSpacing::Paragraph)
}

/// Write a bullet list
fn write_bulletlist(
    list: &BulletList,
    buf: &mut dyn Write,
    config: &AnsiConfig,
    list_depth: usize,
    indent_chars: usize,
    ansi_ctx: &mut AnsiWriterContext,
) -> std::io::Result<LastBlockSpacing> {
    // Determine if list is tight (all first blocks are Plain) or loose
    let is_tight = list
        .content
        .iter()
        .all(|item| !item.is_empty() && matches!(item[0], Block::Plain(_)));

    for (i, item) in list.content.iter().enumerate() {
        // Add blank line between items in loose lists
        if i > 0 && !is_tight {
            writeln!(buf)?;
        }

        // Create context for this item (adds 3 characters: "*, ", "-  ", or "+  ")
        let mut ctx = BulletListContext::new(buf, list_depth, config);

        // Write all blocks in item through the context
        for block in item {
            write_block_with_depth(
                block,
                &mut ctx,
                config,
                list_depth + 1,
                indent_chars + 3,
                ansi_ctx,
            )?;
        }

        ctx.flush()?;
    }

    Ok(LastBlockSpacing::Paragraph)
}

/// Write an ordered list
fn write_orderedlist(
    list: &OrderedList,
    buf: &mut dyn Write,
    config: &AnsiConfig,
    list_depth: usize,
    indent_chars: usize,
    ansi_ctx: &mut AnsiWriterContext,
) -> std::io::Result<LastBlockSpacing> {
    // ListAttributes is (start_number, number_style, number_delim)
    let start_number = list.attr.0;

    // Calculate indent width based on maximum item number
    let indent_width = calculate_indent_width(start_number, list.content.len());

    // Determine if list is tight (all first blocks are Plain) or loose
    let is_tight = list
        .content
        .iter()
        .all(|item| !item.is_empty() && matches!(item[0], Block::Plain(_)));

    for (i, item) in list.content.iter().enumerate() {
        // Add blank line between items in loose lists
        if i > 0 && !is_tight {
            writeln!(buf)?;
        }

        let item_num = start_number + i;
        let mut ctx = OrderedListContext::new(buf, item_num, indent_width, config);

        // Write all blocks in item through the context
        for block in item {
            write_block_with_depth(
                block,
                &mut ctx,
                config,
                list_depth + 1,
                indent_chars + indent_width,
                ansi_ctx,
            )?;
        }

        ctx.flush()?;
    }

    Ok(LastBlockSpacing::Paragraph)
}

/// Write a definition list
fn write_definitionlist(
    list: &DefinitionList,
    buf: &mut dyn Write,
    config: &AnsiConfig,
    list_depth: usize,
    indent_chars: usize,
    ansi_ctx: &mut AnsiWriterContext,
) -> std::io::Result<LastBlockSpacing> {
    let mut style_ctx = StyleContext::new();

    for (i, (term, definitions)) in list.content.iter().enumerate() {
        // Add blank line between definition list items (except before first)
        if i > 0 {
            writeln!(buf)?;
        }

        // Write the term in bold
        if config.colors {
            let term_str = format_inlines(term, config, &mut style_ctx);
            write!(buf, "{}", term_str.bold())?;
        } else {
            write_inlines(term, buf, config, &mut style_ctx)?;
        }
        writeln!(buf)?;

        // Write each definition with ": " prefix
        for definition in definitions {
            // Write the colon in muted color (dark grey)
            if config.colors {
                write!(buf, "{}", ":".dark_grey())?;
            } else {
                write!(buf, ":")?;
            }
            write!(buf, " ")?;

            // Write the definition blocks with proper spacing between them
            let mut last_spacing = LastBlockSpacing::None;
            for (j, block) in definition.iter().enumerate() {
                // Write the block to a buffer first so we can control indentation
                let mut def_buf = Vec::new();
                let block_spacing = write_block_with_depth(
                    block,
                    &mut def_buf,
                    config,
                    list_depth + 1,
                    indent_chars + 2,
                    ansi_ctx,
                )?;

                if j > 0 {
                    // Determine if we need a blank line before this block
                    let needs_blank = match (&last_spacing, block) {
                        (LastBlockSpacing::Plain, Block::Plain(_)) => false,
                        (LastBlockSpacing::None, _) => false,
                        _ => true,
                    };

                    if needs_blank {
                        writeln!(buf)?; // Extra newline for blank line
                    }
                    write!(buf, "  ")?; // Indent continuation
                }

                // Write the definition content, trimming trailing newline since we control spacing
                let content = String::from_utf8_lossy(&def_buf);
                let trimmed = content.trim_end();
                write!(buf, "{}", trimmed)?;
                writeln!(buf)?; // Write the natural newline for this block

                last_spacing = block_spacing;
            }
        }
    }

    Ok(LastBlockSpacing::Paragraph)
}

/// Write a block quote with vertical line marker
fn write_blockquote(
    blockquote: &BlockQuote,
    buf: &mut dyn Write,
    config: &AnsiConfig,
    list_depth: usize,
    indent_chars: usize,
    ansi_ctx: &mut AnsiWriterContext,
) -> std::io::Result<LastBlockSpacing> {
    let mut ctx = BlockQuoteContext::new(buf, config);
    let mut last_spacing = LastBlockSpacing::None;

    for (i, block) in blockquote.content.iter().enumerate() {
        // Determine if we need a blank line before this block
        if i > 0 {
            let needs_blank = match (&last_spacing, block) {
                (LastBlockSpacing::Plain, Block::Plain(_)) => false,
                (LastBlockSpacing::None, _) => false,
                _ => true,
            };

            if needs_blank {
                writeln!(&mut ctx)?; // Extra newline for blank line
            }
        }

        // BlockQuoteContext adds 2 characters: "│ "
        last_spacing = write_block_with_depth(
            block,
            &mut ctx,
            config,
            list_depth,
            indent_chars + 2,
            ansi_ctx,
        )?;
    }

    ctx.flush()?;
    Ok(LastBlockSpacing::Paragraph)
}

fn write_inlines<T: Write + ?Sized>(
    inlines: &[Inline],
    buf: &mut T,
    config: &AnsiConfig,
    style_ctx: &mut StyleContext,
) -> std::io::Result<()> {
    for inline in inlines {
        write_inline(inline, buf, config, style_ctx)?;
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

/// Calculate the display width of a string by stripping ANSI escape sequences
/// This is useful for centering and alignment calculations
fn display_width(s: &str) -> usize {
    let mut width = 0;
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Start of an ANSI escape sequence
            // Skip until we find the final character (letter)
            while let Some(&next_ch) = chars.peek() {
                chars.next();
                if next_ch.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            // Regular character - count it
            width += 1;
        }
    }

    width
}

fn write_inline<T: Write + ?Sized>(
    inline: &Inline,
    buf: &mut T,
    config: &AnsiConfig,
    style_ctx: &mut StyleContext,
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
                write!(
                    buf,
                    "{}",
                    format_inlines(&emph.content, config, style_ctx).italic()
                )?;
                // Restore parent colors after italic styling
                style_ctx.restore_current_style(buf)?;
            } else {
                write_inlines(&emph.content, buf, config, style_ctx)?;
            }
        }
        Inline::Underline(underline) => {
            if config.colors {
                write!(
                    buf,
                    "{}",
                    format_inlines(&underline.content, config, style_ctx).underlined()
                )?;
                // Restore parent colors after underline styling
                style_ctx.restore_current_style(buf)?;
            } else {
                write_inlines(&underline.content, buf, config, style_ctx)?;
            }
        }
        Inline::Strong(strong) => {
            if config.colors {
                write!(
                    buf,
                    "{}",
                    format_inlines(&strong.content, config, style_ctx).bold()
                )?;
                // Restore parent colors after bold styling
                style_ctx.restore_current_style(buf)?;
            } else {
                write_inlines(&strong.content, buf, config, style_ctx)?;
            }
        }
        Inline::Strikeout(strikeout) => {
            if config.colors {
                write!(
                    buf,
                    "{}",
                    format_inlines(&strikeout.content, config, style_ctx).crossed_out()
                )?;
                // Restore parent colors after strikeout styling
                style_ctx.restore_current_style(buf)?;
            } else {
                write_inlines(&strikeout.content, buf, config, style_ctx)?;
            }
        }
        Inline::Superscript(superscript) => {
            // No direct superscript support in terminal, just render content
            write!(buf, "^")?;
            write_inlines(&superscript.content, buf, config, style_ctx)?;
        }
        Inline::Subscript(subscript) => {
            // No direct subscript support in terminal, just render content
            write!(buf, "_")?;
            write_inlines(&subscript.content, buf, config, style_ctx)?;
        }
        Inline::SmallCaps(smallcaps) => {
            // No small caps in terminal, just render as-is
            write_inlines(&smallcaps.content, buf, config, style_ctx)?;
        }
        Inline::Quoted(quoted) => {
            use crate::pandoc::QuoteType;
            let (open, close) = match quoted.quote_type {
                QuoteType::SingleQuote => ("'", "'"),
                QuoteType::DoubleQuote => ("\"", "\""),
            };
            write!(buf, "{}", open)?;
            write_inlines(&quoted.content, buf, config, style_ctx)?;
            write!(buf, "{}", close)?;
        }
        Inline::Cite(cite) => {
            // Render citations as plain text for now
            write_inlines(&cite.content, buf, config, style_ctx)?;
        }
        Inline::Code(code) => {
            if config.colors {
                write!(buf, "{}", code.text.as_str().on_dark_grey().white())?;
                // Restore parent colors after code styling
                style_ctx.restore_current_style(buf)?;
            } else {
                write!(buf, "`{}`", code.text)?;
            }
        }
        Inline::Math(math) => {
            if config.colors {
                write!(buf, "{}", math.text.as_str().yellow())?;
                // Restore parent colors after math styling
                style_ctx.restore_current_style(buf)?;
            } else {
                write!(buf, "${}", math.text)?;
            }
        }
        Inline::RawInline(raw) => {
            // Only render raw content if format matches "ansi"
            if raw.format == "ansi" {
                write!(buf, "{}", raw.text)?;
            }
            // Otherwise skip - wrong format for this writer
        }
        Inline::Link(link) => {
            let url = &link.target.0;
            let link_text = format_inlines(&link.content, config, style_ctx);

            if config.hyperlinks && config.colors {
                // OSC 8 hyperlink: \x1b]8;;URL\x1b\\TEXT\x1b]8;;\x1b\\
                write!(buf, "\x1b]8;;{}\x1b\\", url)?;
                write!(buf, "{}", link_text.cyan().underlined())?;
                write!(buf, "\x1b]8;;\x1b\\")?;
                // Restore parent colors after link styling
                style_ctx.restore_current_style(buf)?;
            } else if config.colors {
                // Styled but not clickable
                write!(buf, "{}", link_text.cyan().underlined())?;
                // Restore parent colors after link styling
                style_ctx.restore_current_style(buf)?;
            } else {
                // No colors - show URL in parentheses
                write_inlines(&link.content, buf, config, style_ctx)?;
                write!(buf, " ({})", url)?;
            }
        }
        Inline::Image(image) => {
            // Render images as [Image: alt text]
            write!(buf, "[Image: ")?;
            write_inlines(&image.content, buf, config, style_ctx)?;
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
                // Push colors onto stack
                style_ctx.push_fg(fg_color);
                style_ctx.push_bg(bg_color);

                // Apply the new colors
                let content_str = format_inlines(&span.content, config, style_ctx);

                let styled = match (fg_color, bg_color) {
                    (Some(fg), Some(bg)) => content_str.with(fg).on(bg),
                    (Some(fg), None) => content_str.with(fg),
                    (None, Some(bg)) => content_str.on(bg),
                    (None, None) => unreachable!(), // We checked above
                };

                write!(buf, "{}", styled)?;

                // Pop colors from stack and restore parent
                style_ctx.pop_bg();
                style_ctx.pop_fg();
                style_ctx.restore_current_style(buf)?;
            } else {
                // No color support or no color attrs, just render content
                write_inlines(&span.content, buf, config, style_ctx)?;
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
            write_inlines(&insert.content, buf, config, style_ctx)?;
        }
        Inline::Delete(delete) => {
            // Render deletes as strikethrough if colors enabled
            if config.colors {
                write!(
                    buf,
                    "{}",
                    format_inlines(&delete.content, config, style_ctx).crossed_out()
                )?;
                // Restore parent colors after strikeout styling
                style_ctx.restore_current_style(buf)?;
            } else {
                write_inlines(&delete.content, buf, config, style_ctx)?;
            }
        }
        Inline::Highlight(highlight) => {
            // Render highlights with background color if enabled
            if config.colors {
                write!(
                    buf,
                    "{}",
                    format_inlines(&highlight.content, config, style_ctx)
                        .on_yellow()
                        .black()
                )?;
                // Restore parent colors after highlight styling
                style_ctx.restore_current_style(buf)?;
            } else {
                write_inlines(&highlight.content, buf, config, style_ctx)?;
            }
        }
        Inline::EditComment(_) => {
            // Ignore edit comments in output
        }
    }
    Ok(())
}

/// Helper to format inlines to a string for styling
fn format_inlines(inlines: &[Inline], config: &AnsiConfig, style_ctx: &mut StyleContext) -> String {
    let mut buf = Vec::new();
    write_inlines(inlines, &mut buf, config, style_ctx).unwrap();
    String::from_utf8(buf).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = AnsiConfig::default();
        assert_eq!(config.colors, true);
        // Width should match what detect_terminal_width returns
        let expected_width = AnsiConfig::detect_terminal_width();
        assert_eq!(config.width, expected_width);
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
