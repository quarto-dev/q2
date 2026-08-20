/*
 * qmd.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::attr::is_empty_attr;
use crate::pandoc::block::MetaBlock;
use crate::pandoc::inline::Inline;
use crate::pandoc::list::{ListNumberDelim, ListNumberStyle};
use crate::pandoc::table::{Alignment, Cell, ColWidth, Row, Table};
use crate::pandoc::{
    Block, BlockQuote, BulletList, CodeBlock, DefinitionList, Figure, Header, HorizontalRule,
    LineBlock, OrderedList, Pandoc, Paragraph, Plain, RawBlock, Str,
};
use hashlink::LinkedHashMap;
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use std::io::{self, Write};
use yaml_rust2::{Yaml, YamlEmitter};

/// Delimiter choice for emphasis and strong emphasis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmphasisDelimiter {
    /// Asterisk delimiter (* or **)
    Asterisk,
    /// Underscore delimiter (_ or __)
    Underscore,
}

/// Stack frame tracking emphasis state
#[derive(Debug, Clone)]
pub struct EmphasisStackFrame {
    /// The delimiter used for this emphasis level
    pub delimiter: EmphasisDelimiter,
    /// Whether this is strong emphasis (true) or regular emphasis (false)
    pub is_strong: bool,
}

/// Context for QMD writer, threaded through all write functions
pub struct QmdWriterContext {
    /// Accumulated error messages during writing
    pub errors: Vec<quarto_error_reporting::DiagnosticMessage>,

    /// Stack tracking parent emphasis delimiters to avoid ambiguity.
    /// When writing nested Emph/Strong nodes, we check this stack to
    /// choose delimiters that won't create *** sequences.
    pub emphasis_stack: Vec<EmphasisStackFrame>,

    /// Whether the byte most recently emitted into the output stream is
    /// Unicode-alphanumeric. Consumed by `escape_markdown` when deciding
    /// whether a `Str`-leading `'` needs to be escaped, so the rule can
    /// look across inline boundaries (e.g. a `Code` span's closing
    /// backtick) and not just within a single `Str` body. Updated by
    /// `write_inline` after each inline emits its bytes and reset to
    /// `false` at the start of every block. See bd-nsb9 / issue #205.
    pub prev_emitted_alnum: bool,

    /// When true, `write_str` keeps smart dashes (—, –) as their literal
    /// Unicode characters instead of canonicalizing them to `---`/`--`. Set by
    /// the prose-block writers for an output line that would otherwise become a
    /// thematic break (see `line_is_dash_only_hazard`); on such a line the
    /// caller emits a leading `\` and the dashes stay Unicode so the reader
    /// recovers them rather than reading a HorizontalRule.
    pub suppress_dash_canonicalization: bool,
}

impl Default for QmdWriterContext {
    fn default() -> Self {
        Self::new()
    }
}

impl QmdWriterContext {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            emphasis_stack: Vec::new(),
            prev_emitted_alnum: false,
            suppress_dash_canonicalization: false,
        }
    }

    pub fn push_emphasis(&mut self, delimiter: EmphasisDelimiter, is_strong: bool) {
        self.emphasis_stack.push(EmphasisStackFrame {
            delimiter,
            is_strong,
        });
    }

    pub fn pop_emphasis(&mut self) {
        self.emphasis_stack.pop();
    }

    /// Choose delimiter for Emph to avoid *** ambiguity
    pub fn choose_emph_delimiter(&self) -> EmphasisDelimiter {
        // If parent uses asterisks (regardless of whether it's Emph or Strong),
        // use underscore to avoid creating *** sequences
        if let Some(parent) = self.emphasis_stack.last()
            && matches!(parent.delimiter, EmphasisDelimiter::Asterisk)
        {
            return EmphasisDelimiter::Underscore;
        }
        EmphasisDelimiter::Asterisk // Default
    }

    /// Choose delimiter for Strong to avoid *** ambiguity
    pub fn choose_strong_delimiter(&self) -> EmphasisDelimiter {
        // If parent uses asterisks (regardless of whether it's Emph or Strong),
        // use underscore to avoid creating *** sequences
        if let Some(parent) = self.emphasis_stack.last()
            && matches!(parent.delimiter, EmphasisDelimiter::Asterisk)
        {
            return EmphasisDelimiter::Underscore;
        }
        EmphasisDelimiter::Asterisk // Default
    }
}

struct BlockQuoteContext<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    at_line_start: bool,
}

impl<'a, W: Write + ?Sized> BlockQuoteContext<'a, W> {
    fn new(inner: &'a mut W) -> Self {
        Self {
            inner,
            at_line_start: true,
        }
    }
}

impl<'a, W: Write + ?Sized> Write for BlockQuoteContext<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut written = 0;
        for &byte in buf {
            if self.at_line_start {
                self.inner.write_all(b"> ")?;
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

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

struct BulletListContext<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    at_line_start: bool,
    is_first_line: bool,
}

impl<'a, W: Write + ?Sized> BulletListContext<'a, W> {
    fn new(inner: &'a mut W) -> Self {
        Self {
            inner,
            at_line_start: true,
            is_first_line: true,
        }
    }
}

impl<'a, W: Write + ?Sized> Write for BulletListContext<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut written = 0;
        for &byte in buf {
            if self.at_line_start {
                if self.is_first_line {
                    self.inner.write_all(b"* ")?;
                    self.is_first_line = false;
                } else {
                    self.inner.write_all(b"  ")?;
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

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

struct OrderedListContext<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    at_line_start: bool,
    is_first_line: bool,
    number: usize,
    number_style: ListNumberStyle,
    delimiter: ListNumberDelim,
    indent: String,
}

impl<'a, W: Write + ?Sized> OrderedListContext<'a, W> {
    fn new(
        inner: &'a mut W,
        number: usize,
        number_style: ListNumberStyle,
        delimiter: ListNumberDelim,
    ) -> Self {
        // Pandoc uses consistent spacing: for numbers < 10, uses two spaces after delimiter
        // For numbers >= 10, uses one space. Continuation lines always use 4 spaces indent.
        let indent = "    ".to_string(); // Always 4 spaces for continuation lines

        Self {
            inner,
            at_line_start: true,
            is_first_line: true,
            number,
            number_style,
            delimiter,
            indent,
        }
    }
}

impl<'a, W: Write + ?Sized> Write for OrderedListContext<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut written = 0;
        for &byte in buf {
            if self.at_line_start {
                if self.is_first_line {
                    // For example lists, always use (@) marker
                    if matches!(self.number_style, ListNumberStyle::Example) {
                        write!(self.inner, "(@)  ")?;
                    } else {
                        let delim_str = match self.delimiter {
                            ListNumberDelim::Period => ".",
                            ListNumberDelim::OneParen => ")",
                            ListNumberDelim::TwoParens => ")",
                            _ => ".",
                        };
                        // Pandoc style: numbers < 10 get two spaces after delimiter,
                        // numbers >= 10 get one space
                        if self.number < 10 {
                            write!(self.inner, "{}{}  ", self.number, delim_str)?;
                        } else {
                            write!(self.inner, "{}{} ", self.number, delim_str)?;
                        }
                    }
                    self.is_first_line = false;
                } else {
                    self.inner.write_all(self.indent.as_bytes())?;
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

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Convert a ConfigValue to a yaml_rust2::Yaml value
/// Phase 5: Works directly with ConfigValue without MetaValueWithSourceInfo conversion
/// PandocInlines and PandocBlocks are rendered using the qmd writer
fn config_value_to_yaml(value: &ConfigValue) -> std::io::Result<Yaml> {
    match &value.value {
        ConfigValueKind::Scalar(yaml) => {
            // Pass through the yaml value directly
            Ok(yaml.clone())
        }
        ConfigValueKind::PandocInlines(content) => {
            // Render inlines using the qmd writer
            let mut buffer = Vec::<u8>::new();
            let mut ctx = QmdWriterContext::new(); // Errors in metadata inlines are unexpected
            for inline in content {
                write_inline(inline, &mut buffer, &mut ctx)?;
            }
            // If any errors accumulated, this is truly unexpected in metadata
            if !ctx.errors.is_empty() {
                return Err(std::io::Error::other(format!(
                    "Unexpected error in metadata inline: {}",
                    ctx.errors[0].title
                )));
            }
            let result = String::from_utf8(buffer)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            Ok(Yaml::String(result))
        }
        ConfigValueKind::PandocBlocks(content) => {
            // Render blocks using the qmd writer
            let mut buffer = Vec::<u8>::new();
            let mut ctx = QmdWriterContext::new(); // Errors in metadata blocks are unexpected
            for (i, block) in content.iter().enumerate() {
                if i > 0 {
                    writeln!(&mut buffer)?;
                }
                write_block(block, &mut buffer, &mut ctx)?;
            }
            // If any errors accumulated, this is truly unexpected in metadata
            if !ctx.errors.is_empty() {
                return Err(std::io::Error::other(format!(
                    "Unexpected error in metadata block: {}",
                    ctx.errors[0].title
                )));
            }
            let result = String::from_utf8(buffer)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            // Trim trailing newline to avoid extra spacing in YAML
            let trimmed = result.trim_end();
            Ok(Yaml::String(trimmed.to_string()))
        }
        ConfigValueKind::Array(items) => {
            let mut yaml_list = Vec::new();
            for item in items {
                yaml_list.push(config_value_to_yaml(item)?);
            }
            Ok(Yaml::Array(yaml_list))
        }
        ConfigValueKind::Map(entries) => {
            // LinkedHashMap preserves insertion order
            let mut yaml_map = LinkedHashMap::new();
            for entry in entries {
                yaml_map.insert(
                    Yaml::String(entry.key.clone()),
                    config_value_to_yaml(&entry.value)?,
                );
            }
            Ok(Yaml::Hash(yaml_map))
        }
        // Path, Glob, and Expr are special types - serialize as their string representation
        ConfigValueKind::Path(p) => Ok(Yaml::String(p.clone())),
        ConfigValueKind::Glob(g) => Ok(Yaml::String(g.clone())),
        ConfigValueKind::Expr(e) => Ok(Yaml::String(format!("!expr {}", e))),
    }
}

/// Phase 5: Write ConfigValue metadata directly to YAML format
fn write_config_value_meta<T: std::io::Write + ?Sized>(
    meta: &ConfigValue,
    buf: &mut T,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<bool> {
    // meta should be a Map variant
    match &meta.value {
        ConfigValueKind::Map(entries) => {
            if entries.is_empty() {
                Ok(false)
            } else {
                // Convert ConfigValue entries to YAML
                // LinkedHashMap preserves insertion order
                let mut yaml_map = LinkedHashMap::new();
                for entry in entries {
                    yaml_map.insert(
                        Yaml::String(entry.key.clone()),
                        config_value_to_yaml(&entry.value)?,
                    );
                }
                let yaml = Yaml::Hash(yaml_map);

                // Emit YAML to string
                let mut yaml_str = String::new();
                let mut emitter = YamlEmitter::new(&mut yaml_str);
                emitter.dump(&yaml).map_err(std::io::Error::other)?;

                // The YamlEmitter adds "---\n" at the start and includes the content
                // We need to add the closing "---\n"
                // First, ensure yaml_str ends with a newline
                if !yaml_str.ends_with('\n') {
                    yaml_str.push('\n');
                }

                // Write the YAML metadata block
                write!(buf, "{}", yaml_str)?;
                writeln!(buf, "---")?;

                Ok(true)
            }
        }
        _ => {
            // Defensive error: metadata should always be a Map
            ctx.errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Invalid metadata structure",
                )
                .with_code("Q-3-40")
                .problem("Metadata must be a map structure for QMD output")
                .add_detail(
                    "The document metadata is not in the expected map format. \
                     This may indicate a filter or processing issue.",
                )
                .add_hint("Check that filters maintain proper metadata structure")
                .build(),
            );
            Ok(false)
        }
    }
}

fn escape_quotes(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The `{#id}` shorthand token is `[#][._A-Za-z0-9-]+` in the grammar; an
/// identifier with any other character (e.g. a slash) can only be written in
/// the `id="..."` key-value form, which the reader promotes back into the
/// identifier slot.
fn id_expressible_as_shorthand(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn write_attr<W: std::io::Write + ?Sized>(
    attr: &crate::pandoc::Attr,
    writer: &mut W,
    _ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    let (id, classes, keyvals) = attr;
    let mut wrote_something = false;
    write!(writer, "{{")?;
    if id_expressible_as_shorthand(id) {
        write!(writer, "#{}", id)?;
        wrote_something = true;
    }
    for class in classes {
        if wrote_something {
            write!(writer, " ")?;
        }
        write!(writer, ".{}", class)?;
        wrote_something = true;
    }
    // The grammar orders attr components id → classes → key-values, so an
    // id that needs the kv fallback goes here, after the classes.
    if !id.is_empty() && !id_expressible_as_shorthand(id) {
        if wrote_something {
            write!(writer, " ")?;
        }
        write!(writer, "id=\"{}\"", escape_quotes(id))?;
        wrote_something = true;
    }
    for (key, value) in keyvals {
        if wrote_something {
            write!(writer, " ")?;
        }
        write!(writer, "{}=\"{}\"", key, escape_quotes(value))?;
        wrote_something = true;
    }
    write!(writer, "}}")?;
    Ok(())
}

/// Like `write_attr`, but for the two attr sites (code fences, inline
/// code spans) where the parser encodes a space-separated `{lang .cls}`
/// language token as a literal bracket-wrapped class string
/// (`"{python}"` — see `test_code_block_attributes.rs` and
/// `engine_cell_lang` in `quarto-core::engine::capture_splice`, the
/// reader-side unwrap this mirrors). Such a class is written back as a
/// bare, unprefixed token instead of being dot-prefixed like a normal
/// class, so `{python .marimo}` round-trips instead of becoming
/// `{.{python} .marimo}`.
///
/// Divs/spans/links/images/tables never receive bracket-wrapped classes
/// from the parser, so they keep using plain `write_attr`.
fn write_code_attr<W: std::io::Write + ?Sized>(
    attr: &crate::pandoc::Attr,
    writer: &mut W,
    _ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    let (id, classes, keyvals) = attr;
    let mut wrote_something = false;
    write!(writer, "{{")?;
    if id_expressible_as_shorthand(id) {
        write!(writer, "#{}", id)?;
        wrote_something = true;
    }
    for class in classes {
        if wrote_something {
            write!(writer, " ")?;
        }
        if let Some(lang) = class.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            write!(writer, "{}", lang)?;
        } else {
            write!(writer, ".{}", class)?;
        }
        wrote_something = true;
    }
    // The grammar orders attr components id → classes → key-values, so an
    // id that needs the kv fallback goes here, after the classes.
    if !id.is_empty() && !id_expressible_as_shorthand(id) {
        if wrote_something {
            write!(writer, " ")?;
        }
        write!(writer, "id=\"{}\"", escape_quotes(id))?;
        wrote_something = true;
    }
    for (key, value) in keyvals {
        if wrote_something {
            write!(writer, " ")?;
        }
        write!(writer, "{}=\"{}\"", key, escape_quotes(value))?;
        wrote_something = true;
    }
    write!(writer, "}}")?;
    Ok(())
}

fn write_blockquote(
    blockquote: &BlockQuote,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    let mut blockquote_writer = BlockQuoteContext::new(buf);
    for (i, block) in blockquote.content.iter().enumerate() {
        if i > 0 {
            // Add a blank line between blocks in the blockquote
            writeln!(&mut blockquote_writer)?;
        }
        write_block(block, &mut blockquote_writer, ctx)?;
    }
    Ok(())
}

fn write_div(
    div: &crate::pandoc::Div,
    writer: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(writer, "::: ")?;
    write_attr(&div.attr, writer, ctx)?;
    writeln!(writer)?;

    for block in div.content.iter() {
        // Add a blank line between blocks in the blockquote
        writeln!(writer)?;
        write_block(block, writer, ctx)?;
    }
    writeln!(writer, "\n:::")?;

    Ok(())
}

/// Pandoc's `taskListItemToAscii`: when a list item's first block is a
/// `Plain`/`Paragraph` starting with `Str "☐"`/`Str "☒"` + `Space` (the
/// task-list convention the reader produces), rewrite the ballot-box `Str`
/// into `RawInline("markdown", "[ ]"/"[x]")` so the qmd writer round-trips
/// the source syntax instead of emitting the Unicode character. The raw
/// inline inherits the marker's source-info (the original bracket bytes).
fn task_item_to_ascii(item: &[Block]) -> Option<Vec<Block>> {
    let content = match item.first()? {
        Block::Plain(p) => &p.content,
        Block::Paragraph(p) => &p.content,
        _ => return None,
    };
    let crate::pandoc::Inline::Str(s) = content.first()? else {
        return None;
    };
    let ascii = match s.text.as_str() {
        "☐" => "[ ]",
        "☒" => "[x]",
        _ => return None,
    };
    if !matches!(content.get(1), Some(crate::pandoc::Inline::Space(_))) {
        return None;
    }
    let marker = crate::pandoc::Inline::RawInline(crate::pandoc::RawInline {
        format: "markdown".to_string(),
        text: ascii.to_string(),
        source_info: s.source_info.clone(),
    });
    let mut rewritten: Vec<Block> = item.to_vec();
    match &mut rewritten[0] {
        Block::Plain(p) => p.content[0] = marker,
        Block::Paragraph(p) => p.content[0] = marker,
        _ => unreachable!("guarded above"),
    }
    Some(rewritten)
}

fn write_bulletlist(
    bulletlist: &BulletList,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    // A list is tight if every non-empty item starts with Plain (not Para).
    // Length-0 items (Vec<Block> = []) carry no spacing information and
    // are skipped here, so a list with a trailing empty item stays tight
    // when its other items are tight.
    let is_tight = bulletlist
        .content
        .iter()
        .all(|item| item.is_empty() || matches!(item[0], Block::Plain(_)));

    for (i, item) in bulletlist.content.iter().enumerate() {
        if i > 0 && !is_tight {
            // Add blank line between items in loose lists
            writeln!(buf)?;
        }

        if item.is_empty() {
            // Length-0 item (Vec<Block> = []). The reader parses a bare
            // `*` (or `-`) marker line as an item with zero blocks; that
            // is a distinct AST shape from `[Plain []]` handled below
            // (`* []`, which parses as an inline `[]` inside Plain).
            writeln!(buf, "*")?;
            continue;
        }

        // Check for the `[Plain []]` / `[Paragraph []]` shape: a single
        // block whose inline content is empty. The reader recovers this
        // from `* []` (inline `[]` parsed as text inside Plain).
        let is_empty_item = item.len() == 1
            && match &item[0] {
                Block::Plain(plain) => plain.content.is_empty(),
                Block::Paragraph(para) => para.content.is_empty(),
                _ => false,
            };

        if is_empty_item {
            writeln!(buf, "* []")?;
        } else {
            let task_rewritten = task_item_to_ascii(item);
            let item: &[Block] = task_rewritten.as_deref().unwrap_or(item);
            let mut item_writer = BulletListContext::new(buf);
            for (j, block) in item.iter().enumerate() {
                if j > 0 && !is_tight {
                    // Add a blank line between blocks within a list item in loose lists
                    writeln!(&mut item_writer)?;
                }
                write_block(block, &mut item_writer, ctx)?;
            }
        }
    }
    Ok(())
}

fn write_orderedlist(
    orderedlist: &OrderedList,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    let (start_num, number_style, delimiter) = &orderedlist.attr;

    // See write_bulletlist: empty items don't carry spacing info.
    let is_tight = orderedlist
        .content
        .iter()
        .all(|item| item.is_empty() || matches!(item[0], Block::Plain(_)));

    for (i, item) in orderedlist.content.iter().enumerate() {
        if i > 0 && !is_tight {
            // Add blank line between items in loose lists
            writeln!(buf)?;
        }
        let current_num = start_num + i;

        if item.is_empty() {
            // Length-0 item: emit a bare marker line (e.g. `1.`) so the
            // reader recovers the empty Vec<Block>. Matches the bullet
            // list bare-marker behavior — see write_bulletlist.
            if matches!(number_style, ListNumberStyle::Example) {
                writeln!(buf, "(@)")?;
            } else {
                let delim_str = match delimiter {
                    ListNumberDelim::Period => ".",
                    ListNumberDelim::OneParen => ")",
                    ListNumberDelim::TwoParens => ")",
                    _ => ".",
                };
                writeln!(buf, "{}{}", current_num, delim_str)?;
            }
            continue;
        }

        let task_rewritten = task_item_to_ascii(item);
        let item: &[Block] = task_rewritten.as_deref().unwrap_or(item);
        let mut item_writer =
            OrderedListContext::new(buf, current_num, number_style.clone(), delimiter.clone());
        for (j, block) in item.iter().enumerate() {
            if j > 0 && !is_tight {
                // Add a blank line between blocks within a list item in loose lists
                writeln!(&mut item_writer)?;
            }
            write_block(block, &mut item_writer, ctx)?;
        }
    }
    Ok(())
}

fn write_header(
    header: &Header,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    // Write the appropriate number of # symbols for the heading level
    for _ in 0..header.level {
        write!(buf, "#")?;
    }
    write!(buf, " ")?;

    // Write the header content
    for inline in &header.content {
        write_inline(inline, buf, ctx)?;
    }

    // Compute the effective attr for writing: suppress auto-generated IDs.
    // When AttrSourceInfo.id is None, the ID was auto-generated by postprocessing.
    // If it matches what auto_generated_id would produce, omit it from the output
    // since the reader will regenerate it on re-parse.
    let effective_attr = if !header.attr.0.is_empty()
        && header.attr_source.id.is_none()
        && header.attr.0 == crate::utils::autoid::auto_generated_id(&header.content)
    {
        (String::new(), header.attr.1.clone(), header.attr.2.clone())
    } else {
        header.attr.clone()
    };

    if !is_empty_attr(&effective_attr) {
        write!(buf, " ")?;
        write_attr(&effective_attr, buf, ctx)?;
    }

    writeln!(buf)?;
    Ok(())
}

// FIXME this is wrong because pipe tables are quite limited (cannot have newlines in them)
fn write_cell_content(
    cell: &Cell,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    for (i, block) in cell.content.iter().enumerate() {
        if i > 0 {
            write!(buf, " ")?; // Join multiple blocks with space
        }
        write_block(block, buf, ctx)?;
    }
    Ok(())
}

fn get_alignment_char(alignment: &Alignment) -> char {
    match alignment {
        Alignment::Left => ':',
        Alignment::Center => ':',
        Alignment::Right => ':',
        Alignment::Default => '-',
    }
}

fn write_codeblock(
    codeblock: &CodeBlock,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    // Determine the number of backticks needed
    // Use at least 3, but more if the content contains backticks
    let fence = determine_backticks(&codeblock.text);
    let fence_length = fence.len().max(3);

    // Write opening fence (always use backticks)
    for _ in 0..fence_length {
        write!(buf, "`")?;
    }

    // Write language/attributes if they exist
    let (id, classes, keyvals) = &codeblock.attr;

    // Only write language as bare word if it's a single class with no other attributes
    if classes.len() == 1 && id.is_empty() && keyvals.is_empty() {
        // Single class, no other attributes: write as bare word
        write!(buf, "{}", classes[0])?;
    } else if !id.is_empty() || !classes.is_empty() || !keyvals.is_empty() {
        // Has attributes: write full attribute block (no space before it).
        // Uses write_code_attr (not write_attr) because a class here may
        // be the bracket-wrapped language pseudo-class — see its doc
        // comment.
        write_code_attr(&codeblock.attr, buf, ctx)?;
    }

    writeln!(buf)?;

    // Write the code content. The reader (matching Pandoc) joins content
    // lines with `\n` and emits no trailing newline, so we always emit a
    // separator newline before the closing fence for non-empty content. If
    // `text` itself ends in `\n`, that newline is preserved as a content-line
    // boundary and our extra writeln keeps the closing fence on its own line,
    // round-tripping the trailing-blank-line case (issue #173).
    write!(buf, "{}", codeblock.text)?;
    if !codeblock.text.is_empty() {
        writeln!(buf)?;
    }

    // Write closing fence (always use backticks)
    for _ in 0..fence_length {
        write!(buf, "`")?;
    }
    writeln!(buf)?;

    Ok(())
}

fn write_lineblock(
    lineblock: &LineBlock,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    for (i, line) in lineblock.content.iter().enumerate() {
        if i > 0 {
            writeln!(buf)?;
        }
        write!(buf, "| ")?;
        for inline in line {
            write_inline(inline, buf, ctx)?;
        }
    }
    writeln!(buf)?;
    Ok(())
}

fn write_rawblock(rawblock: &RawBlock, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    // Only output raw content if it's for markdown format
    if rawblock.format == "markdown" {
        write!(buf, "{}", rawblock.text)?;
    } else {
        // For other formats, use fenced raw block notation with `=` prefix
        writeln!(buf, "```{{={}}}", rawblock.format)?;
        write!(buf, "{}", rawblock.text)?;
        if !rawblock.text.ends_with('\n') {
            writeln!(buf)?;
        }
        writeln!(buf, "```")?;
    }
    Ok(())
}

// INCREMENTAL WRITER COUPLING: This is the sugar transform for definition lists. The
// incremental writer always fully rewrites DefinitionList blocks. See: postprocess.rs
// transform registry.
fn write_definitionlist(
    deflist: &DefinitionList,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    for (i, (term, definitions)) in deflist.content.iter().enumerate() {
        if i > 0 {
            writeln!(buf)?;
        }

        // Write the term
        for inline in term {
            write_inline(inline, buf, ctx)?;
        }
        writeln!(buf)?;

        // Write the definitions
        for definition in definitions {
            write!(buf, ":   ")?;
            for (j, block) in definition.iter().enumerate() {
                if j > 0 {
                    writeln!(buf)?;
                    write!(buf, "    ")?; // Indent subsequent blocks in definition
                }
                write_block(block, buf, ctx)?;
            }
        }
    }
    Ok(())
}

fn write_horizontalrule(
    _rule: &HorizontalRule,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    writeln!(buf, "---")?;
    Ok(())
}

/// Detect the "implicit figure" shape produced by the qmd reader's
/// single-image-paragraph desugaring (postprocess.rs:884). Such a Figure
/// has caption == image alt text, a single `Plain[Image]` content block,
/// and an attr-split where the figure carries only an id and the image
/// carries only classes/kvs. When this exact shape holds we can round-
/// trip by emitting just the bare image syntax with the figure's id
/// merged into the image's attr.
fn match_implicit_figure_shape(figure: &Figure) -> Option<&crate::pandoc::Image> {
    let [Block::Plain(plain)] = figure.content.as_slice() else {
        return None;
    };
    let [Inline::Image(image)] = plain.content.as_slice() else {
        return None;
    };
    if figure.caption.short.is_some() {
        return None;
    }
    let long = figure.caption.long.as_ref()?;
    let [Block::Plain(caption_plain)] = long.as_slice() else {
        return None;
    };
    if caption_plain.content != image.content {
        return None;
    }
    let (_fig_id, fig_classes, fig_kvs) = &figure.attr;
    if !fig_classes.is_empty() || !fig_kvs.is_empty() {
        return None;
    }
    let (img_id, _, _) = &image.attr;
    if !img_id.is_empty() {
        return None;
    }
    Some(image)
}

fn write_figure(
    figure: &Figure,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    if let Some(image) = match_implicit_figure_shape(figure) {
        let mut merged = image.clone();
        merged.attr.0 = figure.attr.0.clone();
        write_image(&merged, buf, ctx)?;
        // Block-writer contract: every block ends with a trailing newline so
        // the inter-block separator emitted by write_impl / write_div produces
        // a blank line. write_image is an inline writer and does not append
        // one. Issue #180 / bd-cpzp.
        writeln!(buf)?;
        return Ok(());
    }

    // Non-implicit shapes fall through to a fenced-div form. The current
    // reader has no rule that turns this back into a Figure, so this
    // branch does not round-trip — tracked in bd-emr4 (follow-up).
    write!(buf, "::: ")?;
    write_attr(&figure.attr, buf, ctx)?;
    writeln!(buf)?;

    // Write the figure content
    for block in &figure.content {
        writeln!(buf)?;
        write_block(block, buf, ctx)?;
    }

    // Write caption if it exists
    if let Some(ref long_caption) = figure.caption.long {
        if !long_caption.is_empty() {
            writeln!(buf)?;
            for (i, block) in long_caption.iter().enumerate() {
                if i > 0 {
                    writeln!(buf)?;
                }
                write_block(block, buf, ctx)?;
            }
        }
    } else if let Some(ref short_caption) = figure.caption.short
        && !short_caption.is_empty()
    {
        writeln!(buf)?;
        // Convert short caption (inlines) to a paragraph for consistency
        for inline in short_caption {
            write_inline(inline, buf, ctx)?;
        }
        writeln!(buf)?;
    }

    writeln!(buf, "\n:::")?;
    Ok(())
}

fn write_metablock(
    metablock: &MetaBlock,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<bool> {
    // Phase 5: Write ConfigValue directly without MetaValueWithSourceInfo conversion
    write_config_value_meta(&metablock.meta, buf, ctx)
}

fn write_inlinerefdef(
    refdef: &crate::pandoc::block::NoteDefinitionPara,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "[^{}]: ", refdef.id)?;
    for inline in &refdef.content {
        write_inline(inline, buf, ctx)?;
    }
    writeln!(buf)?;
    Ok(())
}

fn write_fenced_note_definition(
    refdef: &crate::pandoc::block::NoteDefinitionFencedBlock,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    writeln!(buf, "::: ^{}", refdef.id)?;
    for (i, block) in refdef.content.iter().enumerate() {
        if i > 0 {
            // Add a blank line between blocks
            writeln!(buf)?;
        }
        write_block(block, buf, ctx)?;
    }
    writeln!(buf, ":::")?;
    Ok(())
}

/// Check if a cell's content is "simple" - suitable for pipe table format.
/// Simple content is a single Plain or Paragraph block with no line breaks.
fn cell_has_simple_content(content: &[Block]) -> bool {
    if content.len() != 1 {
        return false;
    }

    let inlines = match &content[0] {
        Block::Plain(plain) => &plain.content,
        Block::Paragraph(para) => &para.content,
        _ => return false,
    };

    // Check for soft/hard breaks in the content
    !inlines
        .iter()
        .any(|inline| matches!(inline, Inline::SoftBreak(_) | Inline::LineBreak(_)))
}

/// Check if a table can be written in pipe table format.
/// Returns false if the table requires list-table format.
///
/// A table can use pipe format if:
/// - No cells have row_span > 1 or col_span > 1
/// - All cells contain only "simple" content (single Para/Plain without breaks)
fn table_can_use_pipe_format(table: &Table) -> bool {
    // Check header rows
    for row in &table.head.rows {
        for cell in &row.cells {
            if cell.row_span > 1 || cell.col_span > 1 {
                return false;
            }
            if !cell_has_simple_content(&cell.content) {
                return false;
            }
        }
    }

    // Check body rows
    for body in &table.bodies {
        for row in &body.body {
            for cell in &row.cells {
                if cell.row_span > 1 || cell.col_span > 1 {
                    return false;
                }
                if !cell_has_simple_content(&cell.content) {
                    return false;
                }
            }
        }
    }

    // Check footer rows
    for row in &table.foot.rows {
        for cell in &row.cells {
            if cell.row_span > 1 || cell.col_span > 1 {
                return false;
            }
            if !cell_has_simple_content(&cell.content) {
                return false;
            }
        }
    }

    true
}

/// Convert Alignment to character code for list-table
fn alignment_to_char(align: &Alignment) -> char {
    match align {
        Alignment::Left => 'l',
        Alignment::Center => 'c',
        Alignment::Right => 'r',
        Alignment::Default => 'd',
    }
}

/// Emit a list-table cell block whose first line continues on the inner
/// bullet's marker line; continuation lines are indented to column 4 so the
/// block stays inside the cell. Used for the first block of a cell when that
/// block is not Plain/Paragraph (e.g. CodeBlock, BlockQuote, Div).
fn write_cell_block_on_marker_line(
    block: &crate::pandoc::Block,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    let mut block_buf = Vec::<u8>::new();
    write_block(block, &mut block_buf, ctx)?;
    let content = String::from_utf8_lossy(&block_buf);
    let mut lines = content.lines();
    if let Some(first_line) = lines.next() {
        writeln!(buf, "{}", first_line)?;
    } else {
        writeln!(buf)?;
    }
    for line in lines {
        if line.is_empty() {
            writeln!(buf)?;
        } else {
            writeln!(buf, "    {}", line)?;
        }
    }
    Ok(())
}

/// Emit a list-table cell block as a 4-space-indented stanza. Used for every
/// block past the first in a cell (the first is handled inline on the marker
/// line by the caller).
fn write_cell_block_indented(
    block: &crate::pandoc::Block,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    let mut block_buf = Vec::<u8>::new();
    write_block(block, &mut block_buf, ctx)?;
    let content = String::from_utf8_lossy(&block_buf);
    for line in content.lines() {
        if line.is_empty() {
            writeln!(buf)?;
        } else {
            writeln!(buf, "    {}", line)?;
        }
    }
    Ok(())
}

/// Write a table as a list-table div.
/// This format supports multi-line cells, rowspan, colspan, etc.
///
/// INCREMENTAL WRITER COUPLING: This is the sugar transform for complex tables. The sugared
/// form uses BulletList (an indentation boundary), so the incremental writer always fully
/// rewrites tables. See: postprocess.rs transform registry.
fn write_list_table(
    table: &Table,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    // Build div attributes
    let mut div_classes = vec!["list-table".to_string()];
    div_classes.extend(table.attr.1.iter().cloned());

    // Count header rows
    let header_row_count = table.head.rows.len();

    // Build attributes
    let mut attrs: Vec<String> = Vec::new();

    // bd-fyb4z: emit `header-rows` whenever it differs from the parser
    // default (1). `header-rows="0"` is essential — without it the
    // re-parse would promote the first row to head. `header-rows="1"`
    // is the default and can be omitted for cleaner output.
    if header_row_count != 1 {
        attrs.push(format!("header-rows=\"{}\"", header_row_count));
    }

    // Add alignments if there are non-default alignments
    let has_non_default_aligns = table.colspec.iter().any(|(a, _)| *a != Alignment::Default);
    if has_non_default_aligns {
        let aligns: String = table
            .colspec
            .iter()
            .map(|(a, _)| alignment_to_char(a).to_string())
            .collect::<Vec<_>>()
            .join(",");
        attrs.push(format!("aligns=\"{}\"", aligns));
    }

    // Add widths if there are non-default widths
    let has_non_default_widths = table
        .colspec
        .iter()
        .any(|(_, w)| matches!(w, ColWidth::Percentage(_)));
    if has_non_default_widths {
        let widths: Vec<String> = table
            .colspec
            .iter()
            .map(|(_, w)| match w {
                ColWidth::Percentage(p) => format!("{}", p),
                ColWidth::Default => "1".to_string(),
            })
            .collect();
        attrs.push(format!("widths=\"{}\"", widths.join(",")));
    }

    // Copy any other attributes from the table
    for (k, v) in &table.attr.2 {
        attrs.push(format!("{}=\"{}\"", k, v));
    }

    // Write div opening
    let id_part = if table.attr.0.is_empty() {
        String::new()
    } else {
        format!("#{} ", table.attr.0)
    };

    let classes_part = div_classes
        .iter()
        .map(|c| format!(".{}", c))
        .collect::<Vec<_>>()
        .join(" ");

    let attrs_part = if attrs.is_empty() {
        String::new()
    } else {
        format!(" {}", attrs.join(" "))
    };

    writeln!(buf, "::: {{{}{}{}}}", id_part, classes_part, attrs_part)?;
    writeln!(buf)?;

    // Write caption if present
    if let Some(ref long_caption) = table.caption.long
        && !long_caption.is_empty()
    {
        for block in long_caption {
            write_block(block, buf, ctx)?;
        }
        writeln!(buf)?;
    }

    // Collect all rows
    let mut all_rows: Vec<&Row> = Vec::new();
    for row in &table.head.rows {
        all_rows.push(row);
    }
    for body in &table.bodies {
        for row in &body.body {
            all_rows.push(row);
        }
    }
    for row in &table.foot.rows {
        all_rows.push(row);
    }

    // Write each row as a bullet list item containing a nested bullet list of cells
    for row in all_rows {
        // Each row is "* - cell1\n  - cell2\n..."
        for (cell_idx, cell) in row.cells.iter().enumerate() {
            if cell_idx == 0 {
                write!(buf, "* ")?;
            } else {
                write!(buf, "  ")?;
            }
            write!(buf, "- ")?;

            // Check if we need to add cell attributes (colspan, rowspan, align)
            let needs_attrs =
                cell.col_span > 1 || cell.row_span > 1 || cell.alignment != Alignment::Default;

            if needs_attrs {
                write!(buf, "[]")?;
                write!(buf, "{{")?;
                let mut attr_parts: Vec<String> = Vec::new();
                if cell.col_span > 1 {
                    attr_parts.push(format!("colspan=\"{}\"", cell.col_span));
                }
                if cell.row_span > 1 {
                    attr_parts.push(format!("rowspan=\"{}\"", cell.row_span));
                }
                if cell.alignment != Alignment::Default {
                    attr_parts.push(format!("align=\"{}\"", alignment_to_char(&cell.alignment)));
                }
                write!(buf, "{}", attr_parts.join(" "))?;
                write!(buf, "}} ")?;
            }

            // Write cell content. Three shapes:
            //   - empty cell without attrs: bare `-` marker line; reader
            //     parses this as a cell with empty Vec<Block>. Writing
            //     literal `[]` here would round-trip to `[Plain []]`
            //     because the reader interprets `[]` as inline text.
            //   - empty cell with attrs: `[]{...attrs}` placeholder so
            //     the attributes parse; the reader produces `[Plain []]`
            //     content in this branch, which round-trips faithfully.
            //   - first block is Plain/Paragraph: inlines on the marker line; any
            //     subsequent blocks emit as blank-line-separated indented stanzas.
            //   - first block is anything else (CodeBlock, BlockQuote, Div, …):
            //     the block's opening line continues the marker line and
            //     continuation lines indent to column 4 — same shape a regular
            //     CommonMark list item uses for non-Plain content.
            // The blank line before each non-first block is what makes the inner
            // list item "loose" so the reader treats indented content as part of
            // the cell.
            if cell.content.is_empty() {
                // When !needs_attrs the bare `- ` token already written
                // at L1125 is enough; no `[]` placeholder.
                writeln!(buf)?;
            } else {
                let (first, rest) = cell.content.split_first().unwrap();
                match first {
                    Block::Plain(p) => {
                        for inline in &p.content {
                            write_inline(inline, buf, ctx)?;
                        }
                        writeln!(buf)?;
                    }
                    Block::Paragraph(p) => {
                        for inline in &p.content {
                            write_inline(inline, buf, ctx)?;
                        }
                        writeln!(buf)?;
                    }
                    _ => {
                        write_cell_block_on_marker_line(first, buf, ctx)?;
                    }
                }
                for block in rest {
                    writeln!(buf)?; // blank-line separator
                    write_cell_block_indented(block, buf, ctx)?;
                }
            }
        }
    }

    writeln!(buf, ":::")?;
    Ok(())
}

// INCREMENTAL WRITER COUPLING: This is the sugar decision point for tables. It chooses
// between pipe table format and list-table div format. The incremental writer always fully
// rewrites Table blocks (never incrementally splices them), so format changes between
// pipe and list-table on rewrite are acceptable (canonicalization, not a bug).
// See: postprocess.rs transform registry.
fn write_table(
    table: &Table,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    // Use list-table format if the table has features that pipe tables don't support
    if !table_can_use_pipe_format(table) {
        return write_list_table(table, buf, ctx);
    }

    // Collect all rows (header + body rows)
    let mut all_rows = Vec::new();

    // Add header rows if they exist
    for row in &table.head.rows {
        all_rows.push(row);
    }

    // Add body rows
    for body in &table.bodies {
        for row in &body.body {
            all_rows.push(row);
        }
    }

    if all_rows.is_empty() {
        return Ok(());
    }

    // Determine number of columns
    let num_cols = table.colspec.len();

    // Extract cell contents as strings for each row
    let mut row_contents: Vec<Vec<String>> = Vec::new();
    let mut max_widths = vec![0; num_cols];

    for row in &all_rows {
        let mut cell_strings = Vec::new();
        for (i, cell) in row.cells.iter().take(num_cols).enumerate() {
            let mut buffer = Vec::<u8>::new();
            write_cell_content(cell, &mut buffer, ctx)?;
            let content = String::from_utf8(buffer)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            let content = content.trim().to_string();

            if content.len() > max_widths[i] {
                max_widths[i] = content.len();
            }
            cell_strings.push(content);
        }
        // Pad to num_cols if needed
        while cell_strings.len() < num_cols {
            cell_strings.push(String::new());
        }
        row_contents.push(cell_strings);
    }

    // Pipe tables require a header line. A Pandoc `TableHead` with zero rows
    // (parser output for tables whose header row was all-empty cells) would
    // otherwise cause the first body row to be emitted as the header and lost
    // on re-parse; insert a synthetic empty header so every body row stays a
    // body row through round-trip.
    if table.head.rows.is_empty() {
        row_contents.insert(0, vec![String::new(); num_cols]);
    }

    // Ensure minimum width of 3 for each column
    for width in &mut max_widths {
        if *width < 3 {
            *width = 3;
        }
    }

    // Write header row (first row)
    if !row_contents.is_empty() {
        write!(buf, "|")?;
        for (i, content) in row_contents[0].iter().enumerate() {
            write!(buf, " {:width$} |", content, width = max_widths[i])?;
        }
        writeln!(buf)?;

        // Write separator line
        write!(buf, "|")?;
        for (i, colspec) in table.colspec.iter().enumerate().take(num_cols) {
            let _align_char = get_alignment_char(&colspec.0);
            let sep = match colspec.0 {
                Alignment::Left => format!(":{}", "-".repeat(max_widths[i] - 1)),
                Alignment::Center => format!(":{}:", "-".repeat(max_widths[i] - 2)),
                Alignment::Right => format!("{}:", "-".repeat(max_widths[i] - 1)),
                Alignment::Default => "-".repeat(max_widths[i]),
            };
            write!(buf, " {} |", sep)?;
        }
        writeln!(buf)?;

        // Write body rows (skip first row which is header)
        for row_content in row_contents.iter().skip(1) {
            write!(buf, "|")?;
            for (i, content) in row_content.iter().enumerate() {
                write!(buf, " {:width$} |", content, width = max_widths[i])?;
            }
            writeln!(buf)?;
        }
    }

    // Write caption if it exists
    if let Some(ref long_caption) = table.caption.long
        && !long_caption.is_empty()
    {
        writeln!(buf)?; // Blank line before caption

        // The table's attribute block is parsed from a trailing {…} on the
        // caption line (see pipe_table.rs:191-195), so on output we attach it
        // to the last caption line that actually emits content.
        let last_emittable_idx = long_caption
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, b)| match b {
                Block::Plain(_) | Block::Paragraph(_) => Some(i),
                _ => None,
            });

        for (i, block) in long_caption.iter().enumerate() {
            let content = match block {
                Block::Plain(plain) => Some(&plain.content),
                Block::Paragraph(para) => Some(&para.content),
                _ => None,
            };
            if let Some(inlines) = content {
                write!(buf, ": ")?;
                for inline in inlines {
                    write_inline(inline, buf, ctx)?;
                }
                if Some(i) == last_emittable_idx && !is_empty_attr(&table.attr) {
                    write!(buf, " ")?;
                    write_attr(&table.attr, buf, ctx)?;
                }
                writeln!(buf)?;
            }
        }
    }

    Ok(())
}

fn determine_backticks(text: &str) -> String {
    // Find the longest sequence of consecutive backticks in the text
    let mut max_backticks = 0;
    let mut current_backticks = 0;

    for ch in text.chars() {
        if ch == '`' {
            current_backticks += 1;
            max_backticks = max_backticks.max(current_backticks);
        } else {
            current_backticks = 0;
        }
    }

    // Use one more backtick than the longest sequence found
    "`".repeat(max_backticks + 1)
}

// True if a single output line composed of these inlines would, after dash
// canonicalization, be misread by the reader as a thematic break
// (HorizontalRule): every inline is either a `Space` or a `Str` made solely of
// dash characters (em —, en –, or ASCII hyphen), and the dashes sum to at least
// three ASCII hyphens. Such a line must keep its dashes as unicode and prefix a
// backslash (`\—…`) instead of canonicalizing to ASCII, so the leading `\`
// stops thematic-break recognition while the reader still recovers the dash.
//
// En dash alone (`--`, 2 hyphens) and a lone hyphen are below the thematic-break
// minimum and stay safe, so they are intentionally *not* flagged.
fn line_is_dash_only_hazard(line: &[Inline]) -> bool {
    let mut hyphens = 0usize;
    for inline in line {
        match inline {
            Inline::Space(_) => {}
            Inline::Str(s) => {
                for c in s.text.chars() {
                    match c {
                        '\u{2014}' => hyphens += 3,
                        '\u{2013}' => hyphens += 2,
                        '-' => hyphens += 1,
                        _ => return false,
                    }
                }
            }
            _ => return false,
        }
    }
    hyphens >= 3
}

// Helper function to canonicalize smart typography and escape special markdown
// characters in a single pass over the *original* `Str` text. This follows
// Pandoc's escaping strategy: escape characters that have special markdown
// meaning so the document round-trips (qmd -> AST -> qmd).
//
// Smart typography canonicalization (`—`→`---`, `–`→`--`, `…`→`...`, `’`→`'`)
// is emitted UNescaped, because the reader re-applies smart typography and turns
// the ASCII spelling back into the Unicode character. Conversely, a *literal*
// ASCII run that the reader's smart pass would otherwise convert — a hyphen run
// of length ≥2 (→ en/em dash) or a dot run of length ≥3 (→ ellipsis) — is
// backslash-escaped per character (`\-`, `\.`) so it stays literal. Working on
// the original text is what lets us tell a canonicalized em dash (`—` → bare
// `---`) apart from genuinely-literal hyphens (`--` → `\-\-`). A run short
// enough to be inert (a lone hyphen, or one/two dots) is emitted verbatim.
//
// The ASCII apostrophe `'` (and its Unicode form `’`) is escaped whenever the
// reader would otherwise misclassify it as a smart-quote open/close. The
// reader's smart-quote classifier accepts `'` as an apostrophe only when it sits
// between two alphanumeric characters (e.g. `don't`, `it's`, `ab'9`); in any
// other position the apostrophe is treated as a quotation mark and produces a
// Q-2-7 / Q-2-10 parse error on the regenerated qmd. The lookup uses the
// previous char in the `Str` body when one exists, otherwise
// `start_prev_is_alnum` (carried in from `QmdWriterContext` so the rule can see
// across inline boundaries — e.g. a closing backtick from a preceding `Code`
// span; see bd-nsb9 / issue #205), and the next char in the `Str` body.
fn escape_markdown(text: &str, start_prev_is_alnum: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            // Characters that must be escaped to avoid triggering markdown syntax:
            '\\' => result.push_str("\\\\"), // Escape character itself
            '<' => result.push_str("\\<"),   // HTML tags, autolinks
            '>' => result.push_str("\\>"),   // Blockquotes (at line start)
            '#' => result.push_str("\\#"),   // Headers (at line start)
            '$' => result.push_str("\\$"),   // Math delimiters
            '*' => result.push_str("\\*"),   // Emphasis, strong, lists
            '_' => result.push_str("\\_"),   // Emphasis, strong
            '[' => result.push_str("\\["),   // Links
            ']' => result.push_str("\\]"),   // Links
            '`' => result.push_str("\\`"),   // Code spans
            '|' => result.push_str("\\|"),   // Tables
            '~' => result.push_str("\\~"),   // Subscript, strikeout
            '^' => result.push_str("\\^"),   // Superscript
            '@' => result.push_str("\\@"),   // Citations: every bare @ in
            // a Str is either a citation start (when followed by alnum/_/{)
            // or an outright parse error (any other position). Always escape.
            '{' => result.push_str("\\{"), // Attribute span open: bare { in
            '}' => result.push_str("\\}"), // a Str body is always a parse
            // error in qmd. Always escape.
            '"' => result.push_str("\\\""), // A bare " in a Str body would be
            // re-read as a smart quote (Quoted DoubleQuote); `\"` re-reads as
            // the literal straight quote. Str bodies only contain " via
            // character references (&quot;, &#34;) or programmatic ASTs —
            // parsed quotation marks become Quoted nodes instead.

            // Smart typography → ASCII source spelling, emitted UNescaped so the
            // reader re-converts it back to the Unicode character.
            '\u{2014}' => result.push_str("---"), // em dash
            '\u{2013}' => result.push_str("--"),  // en dash
            '\u{2026}' => result.push_str("..."), // ellipsis

            // Literal ASCII runs the reader's smart pass would convert: escape
            // per character so they round-trip as literal text.
            '-' => {
                let start = i;
                while i < chars.len() && chars[i] == '-' {
                    i += 1;
                }
                let run = i - start;
                if run >= 2 {
                    for _ in 0..run {
                        result.push_str("\\-");
                    }
                } else {
                    result.push('-');
                }
                continue; // `i` already advanced past the run
            }
            '.' => {
                let start = i;
                while i < chars.len() && chars[i] == '.' {
                    i += 1;
                }
                let run = i - start;
                if run >= 3 {
                    for _ in 0..run {
                        result.push_str("\\.");
                    }
                } else {
                    for _ in 0..run {
                        result.push('.');
                    }
                }
                continue; // `i` already advanced past the run
            }

            '\'' | '\u{2019}' => {
                let prev_is_alnum = if i > 0 {
                    chars[i - 1].is_alphanumeric()
                } else {
                    start_prev_is_alnum
                };
                let next_is_alnum = chars.get(i + 1).is_some_and(|c| c.is_alphanumeric());
                if prev_is_alnum && next_is_alnum {
                    result.push('\'');
                } else {
                    result.push_str("\\'");
                }
            }

            // Characters that don't need escaping in most contexts:
            // , + ! ? = : ; / ( ) % &
            // These are only special in very specific contexts and escaping them
            // everywhere would make output unnecessarily verbose.
            _ => result.push(ch),
        }
        i += 1;
    }
    result
}

fn write_str(
    s: &Str,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    if ctx.suppress_dash_canonicalization {
        // This Str sits on a line that would otherwise canonicalize to only
        // dashes and re-parse as a thematic break (HorizontalRule). Escape each
        // dash character individually so the line cannot be a thematic break,
        // while still round-tripping to the same AST:
        //   * em/en dash kept Unicode + `\` → `\—`, `\–`  (reader: `\X` → X)
        //   * literal hyphen → `\-`                       (reader: `\-` → -)
        // Written directly: routing `\` through `escape_markdown` would double
        // it. Such a line contains only dash chars (see `line_is_dash_only_hazard`),
        // so no other markdown-special characters can appear here.
        for c in s.text.chars() {
            match c {
                '\u{2014}' | '\u{2013}' | '-' => write!(buf, "\\{c}")?,
                '\u{2019}' => write!(buf, "'")?,
                other => write!(buf, "{other}")?,
            }
        }
        return Ok(());
    }
    let escaped = escape_markdown(&s.text, ctx.prev_emitted_alnum);
    write!(buf, "{}", escaped)
}

fn write_space(
    _: &crate::pandoc::Space,
    buf: &mut dyn std::io::Write,
    _ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, " ")
}

fn write_soft_break(
    _: &crate::pandoc::SoftBreak,
    buf: &mut dyn std::io::Write,
    _ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    // Pandoc's writer for markdown outputs a space for soft breaks
    // We choose to deviate from Pandoc for roundtripping purposes
    writeln!(buf)
}

fn write_emph(
    emph: &crate::pandoc::Emph,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    // Choose delimiter based on parent emphasis to avoid *** ambiguity
    let delimiter = ctx.choose_emph_delimiter();
    let delim_str = match delimiter {
        EmphasisDelimiter::Asterisk => "*",
        EmphasisDelimiter::Underscore => "_",
    };

    write!(buf, "{}", delim_str)?;
    ctx.push_emphasis(delimiter, false);

    for inline in &emph.content {
        write_inline(inline, buf, ctx)?;
    }

    ctx.pop_emphasis();
    write!(buf, "{}", delim_str)
}

fn write_strong(
    strong: &crate::pandoc::Strong,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    // Choose delimiter based on parent emphasis to avoid *** ambiguity
    let delimiter = ctx.choose_strong_delimiter();
    let delim_str = match delimiter {
        EmphasisDelimiter::Asterisk => "**",
        EmphasisDelimiter::Underscore => "__",
    };

    write!(buf, "{}", delim_str)?;
    ctx.push_emphasis(delimiter, true);

    for inline in &strong.content {
        write_inline(inline, buf, ctx)?;
    }

    ctx.pop_emphasis();
    write!(buf, "{}", delim_str)
}

fn write_code(
    code: &crate::pandoc::Code,
    buf: &mut dyn std::io::Write,
    _ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    // Handle inline code with proper backtick escaping
    let backticks = determine_backticks(&code.text);
    write!(buf, "{}", backticks)?;
    if code.text.starts_with('`') || code.text.ends_with('`') {
        // Add spaces to prevent backticks from being interpreted as delimiters
        write!(buf, " {} ", code.text)?;
    } else {
        write!(buf, "{}", code.text)?;
    }
    write!(buf, "{}", backticks)?;
    // TODO: Handle attributes if non-empty
    // Uses write_code_attr (not write_attr): inline code shares the
    // parser's bracket-wrapped language-class encoding with code fences
    // — see write_code_attr's doc comment.
    if !is_empty_attr(&code.attr) {
        write_code_attr(&code.attr, buf, _ctx)?;
    }
    Ok(())
}

fn write_linebreak(
    _line_break: &crate::pandoc::LineBreak,
    buf: &mut dyn std::io::Write,
    _ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "\\")?;
    writeln!(buf)
}

/// Check if a link matches the anchor shorthand pattern: `[id](#id){.anchor}`
/// Returns the anchor id if it matches, None otherwise.
fn anchor_shorthand_id(link: &crate::pandoc::Link) -> Option<&str> {
    // Must have exactly one class "anchor", no id, no key-value pairs
    let (ref id, ref classes, ref kvs) = link.attr;
    if !id.is_empty() || classes.len() != 1 || classes[0] != "anchor" || !kvs.is_empty() {
        return None;
    }
    // Must have no title
    if !link.target.1.is_empty() {
        return None;
    }
    // Content must be a single Str node
    if link.content.len() != 1 {
        return None;
    }
    let text = match &link.content[0] {
        crate::pandoc::Inline::Str(s) => &s.text,
        _ => return None,
    };
    // Target must be "#" + the same text
    let target_id = link.target.0.strip_prefix('#')?;
    if target_id != text {
        return None;
    }
    Some(target_id)
}

fn write_link(
    link: &crate::pandoc::Link,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    // Anchor shorthand: [id](#id){.anchor} -> <#id>
    if let Some(anchor_id) = anchor_shorthand_id(link) {
        return write!(buf, "<#{}>", anchor_id);
    }
    write!(buf, "[")?;
    for inline in &link.content {
        write_inline(inline, buf, ctx)?;
    }
    write!(buf, "](")?;
    write!(buf, "{}", link.target.0)?;
    if !link.target.1.is_empty() {
        write!(buf, " \"{}\"", escape_quotes(&link.target.1))?;
    }
    write!(buf, ")")?;
    if !is_empty_attr(&link.attr) {
        write_attr(&link.attr, buf, ctx)?;
    }
    Ok(())
}

fn write_image(
    image: &crate::pandoc::Image,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "![")?;
    for inline in &image.content {
        write_inline(inline, buf, ctx)?;
    }
    write!(buf, "](")?;
    write!(buf, "{}", image.target.0)?;
    if !image.target.1.is_empty() {
        write!(buf, " \"{}\"", escape_quotes(&image.target.1))?;
    }
    write!(buf, ")")?;
    if !is_empty_attr(&image.attr) {
        write_attr(&image.attr, buf, ctx)?;
    }
    Ok(())
}

fn write_strikeout(
    strikeout: &crate::pandoc::Strikeout,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "~~")?;
    for inline in &strikeout.content {
        write_inline(inline, buf, ctx)?;
    }
    write!(buf, "~~")
}

fn write_subscript(
    subscript: &crate::pandoc::Subscript,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "~")?;
    for inline in &subscript.content {
        write_inline(inline, buf, ctx)?;
    }
    write!(buf, "~")
}

fn write_superscript(
    superscript: &crate::pandoc::Superscript,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "^")?;
    for inline in &superscript.content {
        write_inline(inline, buf, ctx)?;
    }
    write!(buf, "^")
}

fn write_math(
    math: &crate::pandoc::Math,
    buf: &mut dyn std::io::Write,
    _ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    match math.math_type {
        crate::pandoc::MathType::InlineMath => {
            write!(buf, "${}$", math.text)?;
        }
        crate::pandoc::MathType::DisplayMath => {
            write!(buf, "$${}$$", math.text)?;
        }
    }
    Ok(())
}

fn write_quoted(
    quoted: &crate::pandoc::Quoted,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    match quoted.quote_type {
        crate::pandoc::QuoteType::SingleQuote => {
            write!(buf, "'")?;
            for inline in &quoted.content {
                write_inline(inline, buf, ctx)?;
            }
            write!(buf, "'")?;
        }
        crate::pandoc::QuoteType::DoubleQuote => {
            write!(buf, "\"")?;
            for inline in &quoted.content {
                write_inline(inline, buf, ctx)?;
            }
            write!(buf, "\"")?;
        }
    }
    Ok(())
}
fn write_span(
    span: &crate::pandoc::Span,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    let (id, classes, keyvals) = &span.attr;

    // Check if this is an editorial mark span that should use decorated syntax
    // These spans have exactly one class, no ID, and no key-value pairs
    if id.is_empty() && classes.len() == 1 && keyvals.is_empty() {
        let marker = match classes[0].as_str() {
            "quarto-highlight" => Some("!! "),
            "quarto-insert" => Some("++ "),
            "quarto-delete" => Some("-- "),
            "quarto-edit-comment" => Some(">> "),
            _ => None,
        };

        if let Some(marker) = marker {
            // Write using decorated syntax
            write!(buf, "[{}", marker)?;
            for inline in &span.content {
                write_inline(inline, buf, ctx)?;
            }
            write!(buf, "]")?;
            return Ok(());
        }
    }

    // Spans use bracket syntax: [content]{#id .class key=value}
    // When attributes are empty, omit the {} suffix — the parser treats [content]
    // as a Span regardless. For empty content + empty attrs, write [ ] with a space
    // for readability and to distinguish from empty-cell [] in list tables.
    write!(buf, "[")?;
    if span.content.is_empty() && is_empty_attr(&span.attr) {
        write!(buf, " ]")?;
        return Ok(());
    }
    for inline in &span.content {
        write_inline(inline, buf, ctx)?;
    }
    write!(buf, "]")?;
    if !is_empty_attr(&span.attr) {
        write_attr(&span.attr, buf, ctx)?;
    }
    Ok(())
}

fn write_underline(
    underline: &crate::pandoc::Underline,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "[")?;
    for inline in &underline.content {
        write_inline(inline, buf, ctx)?;
    }
    write!(buf, "]{{.underline}}")
}
fn write_smallcaps(
    smallcaps: &crate::pandoc::SmallCaps,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "[")?;
    for inline in &smallcaps.content {
        write_inline(inline, buf, ctx)?;
    }
    write!(buf, "]{{.smallcaps}}")
}
fn write_cite(
    cite: &crate::pandoc::Cite,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    use crate::pandoc::CitationMode;

    // Check if we have any NormalCitation or SuppressAuthor citations
    // These need to be wrapped in brackets together
    let has_bracketed = cite.citations.iter().any(|c| {
        matches!(
            c.mode,
            CitationMode::NormalCitation | CitationMode::SuppressAuthor
        )
    });

    if has_bracketed {
        // All citations go in one set of brackets
        write!(buf, "[")?;
        for (i, citation) in cite.citations.iter().enumerate() {
            if i > 0 {
                write!(buf, "; ")?;
            }

            // Write prefix
            let has_prefix = !citation.prefix.is_empty();
            for inline in &citation.prefix {
                write_inline(inline, buf, ctx)?;
            }
            // Only add space if prefix exists and doesn't end with whitespace
            if has_prefix {
                let ends_with_space = citation
                    .prefix
                    .last()
                    .is_some_and(|inline| matches!(inline, crate::pandoc::Inline::Space(_)));
                if !ends_with_space {
                    write!(buf, " ")?;
                }
            }
            // Write the citation itself
            let prefix = if matches!(citation.mode, CitationMode::SuppressAuthor) {
                "-@"
            } else {
                "@"
            };
            write!(buf, "{}{}", prefix, citation.id)?;

            // Write suffix
            for inline in &citation.suffix {
                write_inline(inline, buf, ctx)?;
            }
        }
        write!(buf, "]")?;
    } else {
        // All citations are AuthorInText, write them without brackets
        for (i, citation) in cite.citations.iter().enumerate() {
            if i > 0 {
                write!(buf, "; ")?;
            }
            write!(buf, "@{}", citation.id)?;

            // Write suffix if it exists
            // For AuthorInText mode, suffix appears as: @citation [suffix]
            if !citation.suffix.is_empty() {
                write!(buf, " [")?;
                for inline in &citation.suffix {
                    write_inline(inline, buf, ctx)?;
                }
                write!(buf, "]")?;
            }
        }
    }

    // Note: We don't write cite.content as it's just a rendered representation
    // of what we've already written above
    Ok(())
}
fn write_rawinline(
    raw: &crate::pandoc::RawInline,
    buf: &mut dyn std::io::Write,
    _ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    if raw.format == "markdown" {
        // Markdown raw content is emitted directly
        write!(buf, "{}", raw.text)
    } else if raw.format == "html" && is_html_comment(&raw.text) {
        // HTML comments are emitted in native syntax (bd-1066)
        write!(buf, "{}", raw.text)
    } else {
        // For other formats, use raw span notation with = prefix
        write!(buf, "`{}`{{={}}}", raw.text, raw.format)
    }
}

/// Check if a raw HTML string is an HTML comment (<!-- ... -->).
/// The text may have leading whitespace from the tree-sitter node span.
fn is_html_comment(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("<!--") && trimmed.ends_with("-->")
}
fn write_note(
    note: &crate::pandoc::Note,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "^[")?;
    for (i, block) in note.content.iter().enumerate() {
        if i > 0 {
            write!(buf, " ")?;
        }
        // For inline notes, we need to flatten block content
        match block {
            crate::pandoc::Block::Plain(plain) => {
                for inline in &plain.content {
                    write_inline(inline, buf, ctx)?;
                }
            }
            crate::pandoc::Block::Paragraph(para) => {
                for inline in &para.content {
                    write_inline(inline, buf, ctx)?;
                }
            }
            _ => {
                write!(buf, "[complex block]")?;
            }
        }
    }
    write!(buf, "]")?;
    Ok(())
}
fn write_notereference(
    noteref: &crate::pandoc::NoteReference,
    buf: &mut dyn std::io::Write,
    _ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "[^{}]", noteref.id)
}
/// Serialize a shortcode back to its qmd source form (`{{< … >}}`;
/// escaped: `{{{< … >}}}`), including grammar-accurate arg quoting.
/// Public so non-writer consumers (e.g. quarto-core's include-slot
/// handling) can reconstruct literal shortcode text for later
/// text-level expansion.
pub fn shortcode_source_text(shortcode: &crate::pandoc::Shortcode) -> String {
    let mut buf = Vec::new();
    write_shortcode(shortcode, &mut buf).expect("writing to a Vec cannot fail");
    String::from_utf8(buf).expect("qmd shortcode serialization is UTF-8")
}

fn write_shortcode(
    shortcode: &crate::pandoc::Shortcode,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    let (open, close) = if shortcode.is_escaped {
        ("{{{<", ">}}}")
    } else {
        ("{{<", ">}}")
    };
    write!(buf, "{} {}", open, shortcode.name)?;
    for arg in &shortcode.positional_args {
        write!(buf, " ")?;
        write_shortcode_arg(arg, buf)?;
    }
    for (key, value) in &shortcode.keyword_args {
        write!(buf, " {}=", key)?;
        write_shortcode_arg(value, buf)?;
    }
    write!(buf, " {}", close)
}

fn write_shortcode_arg(
    arg: &crate::pandoc::ShortcodeArg,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    use crate::pandoc::ShortcodeArg;
    match arg {
        ShortcodeArg::String(s) => write_shortcode_string_value(s, buf),
        ShortcodeArg::Number(n) => write!(buf, "{}", n),
        ShortcodeArg::Boolean(b) => write!(buf, "{}", b),
        ShortcodeArg::Shortcode(inner) => write_shortcode(inner, buf),
        ShortcodeArg::KeyValue(map) => {
            // Positional `key=value` pair(s). The qmd parser doesn't currently
            // produce this variant from source — it's reachable only via Lua
            // filters or programmatic construction — so the round-trip
            // contract is best-effort.
            let mut first = true;
            for (k, v) in map {
                if !first {
                    write!(buf, " ")?;
                }
                first = false;
                write!(buf, "{}=", k)?;
                write_shortcode_arg(v, buf)?;
            }
            Ok(())
        }
    }
}

/// Emit a `ShortcodeArg::String` value, quoting on demand. Quoting rules
/// mirror the qmd grammar (`tree-sitter-qmd/tree-sitter-markdown/grammar.js`,
/// `shortcode_naked_string` and `shortcode_number`):
/// - empty string → must be quoted (parser would fail to match)
/// - matches `shortcode_number` exactly → must be quoted (otherwise
///   re-parses as `ShortcodeArg::Number`, changing the AST type)
/// - any character outside the naked-string set → must be quoted
fn write_shortcode_string_value(s: &str, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    if shortcode_string_needs_quoting(s) {
        buf.write_all(b"\"")?;
        for c in s.chars() {
            match c {
                '"' => buf.write_all(b"\\\"")?,
                '\\' => buf.write_all(b"\\\\")?,
                _ => write!(buf, "{}", c)?,
            }
        }
        buf.write_all(b"\"")
    } else {
        buf.write_all(s.as_bytes())
    }
}

fn shortcode_string_needs_quoting(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    if shortcode_string_looks_like_number(s) {
        return true;
    }
    !s.chars().all(is_shortcode_naked_char)
}

/// Characters allowed in a `shortcode_naked_string` token. Mirrors the
/// regex `[A-Za-z0-9_.~:/?#\]@!$%&()+,;-]|\[` from grammar.js. Whitespace,
/// `>`, `<`, `{`, `}`, `'`, `"`, `\`, `*`, `=`, `^`, `|` are deliberately
/// excluded.
fn is_shortcode_naked_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '_' | '.'
                | '~'
                | ':'
                | '/'
                | '?'
                | '#'
                | ']'
                | '@'
                | '!'
                | '$'
                | '%'
                | '&'
                | '('
                | ')'
                | '+'
                | ','
                | ';'
                | '-'
                | '['
        )
}

/// Mirrors the `shortcode_number` regex
/// `-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?` from grammar.js. A string
/// matching this pattern would re-parse as `ShortcodeArg::Number`, so we
/// quote it to keep the AST stable across a write/read round-trip.
fn shortcode_string_looks_like_number(s: &str) -> bool {
    let mut chars = s.chars().peekable();
    if chars.peek() == Some(&'-') {
        chars.next();
    }
    match chars.next() {
        Some('0') => {}
        Some(c) if c.is_ascii_digit() => {
            // c is '1'..='9' — consume remaining digits
            while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                chars.next();
            }
        }
        _ => return false,
    }
    if chars.peek() == Some(&'.') {
        chars.next();
        let mut had = false;
        while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
            chars.next();
            had = true;
        }
        if !had {
            return false;
        }
    }
    if matches!(chars.peek(), Some('e' | 'E')) {
        chars.next();
        if matches!(chars.peek(), Some('+' | '-')) {
            chars.next();
        }
        let mut had = false;
        while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
            chars.next();
            had = true;
        }
        if !had {
            return false;
        }
    }
    chars.next().is_none()
}

#[cfg(test)]
mod smart_typography_writer_tests {
    use super::{escape_markdown, line_is_dash_only_hazard};
    use crate::pandoc::inline::{Inline, Space, Str};
    use quarto_source_map::SourceInfo;

    fn s(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
        })
    }

    fn space() -> Inline {
        Inline::Space(Space {
            source_info: SourceInfo::for_test(),
        })
    }

    #[test]
    fn canonicalizes_unicode_smart_chars_unescaped() {
        // Unicode smart chars become their ASCII spelling, emitted UNescaped so
        // the reader re-converts them.
        assert_eq!(escape_markdown("\u{2014}", false), "---"); // em → ---
        assert_eq!(escape_markdown("\u{2013}", false), "--"); // en → --
        assert_eq!(escape_markdown("\u{2026}", false), "..."); // … → ...
        assert_eq!(escape_markdown("a\u{2014}b\u{2026}c", false), "a---b...c");
        // ’ between alnums is kept as ', elsewhere escaped.
        assert_eq!(escape_markdown("don\u{2019}t", false), "don't");
    }

    #[test]
    fn escapes_literal_dash_and_dot_runs() {
        // Literal ASCII runs the reader would smart-convert must be escaped so
        // they stay literal (e.g. from escaped input `a\-\-b`).
        assert_eq!(escape_markdown("a--b", false), "a\\-\\-b");
        assert_eq!(escape_markdown("a---b", false), "a\\-\\-\\-b");
        assert_eq!(escape_markdown("a-b", false), "a-b"); // lone hyphen inert
        assert_eq!(escape_markdown("x...", false), "x\\.\\.\\.");
        assert_eq!(escape_markdown("x..", false), "x.."); // two dots inert
    }

    #[test]
    fn hazard_detects_all_dash_lines() {
        assert!(line_is_dash_only_hazard(&[s("\u{2014}")])); // — = 3 hyphens
        assert!(line_is_dash_only_hazard(&[s("\u{2013}\u{2013}")])); // –– = 4
        assert!(line_is_dash_only_hazard(&[s("---")])); // literal --- (from escapes)
        assert!(line_is_dash_only_hazard(&[
            s("\u{2014}"),
            space(),
            s("\u{2014}")
        ])); // — — → ------ with space
    }

    #[test]
    fn hazard_ignores_safe_lines() {
        assert!(!line_is_dash_only_hazard(&[s("\u{2013}")])); // – = only 2 hyphens
        assert!(!line_is_dash_only_hazard(&[s("-")])); // 1 hyphen
        assert!(!line_is_dash_only_hazard(&[s("\u{2014}a")])); // — followed by text
        assert!(!line_is_dash_only_hazard(&[s("a"), s("\u{2014}")])); // contains a letter
        assert!(!line_is_dash_only_hazard(&[])); // empty
    }
}

#[cfg(test)]
mod shortcode_writer_tests {
    use super::{shortcode_string_looks_like_number, shortcode_string_needs_quoting};

    #[test]
    fn naked_string_does_not_need_quoting() {
        assert!(!shortcode_string_needs_quoting("foo"));
        assert!(!shortcode_string_needs_quoting("https://youtu.be/abc"));
        assert!(!shortcode_string_needs_quoting("a-b_c.d"));
        // `?` alone is in the naked-string set; `=` is not (the grammar only
        // permits `=` in the second segment after a `?`, and we conservatively
        // quote any `=`).
        assert!(!shortcode_string_needs_quoting("path/to?query"));
    }

    #[test]
    fn empty_must_be_quoted() {
        assert!(shortcode_string_needs_quoting(""));
    }

    #[test]
    fn whitespace_must_be_quoted() {
        assert!(shortcode_string_needs_quoting("foo bar"));
        assert!(shortcode_string_needs_quoting(" leading"));
        assert!(shortcode_string_needs_quoting("trailing "));
        assert!(shortcode_string_needs_quoting("a\tb"));
    }

    #[test]
    fn delimiter_chars_must_be_quoted() {
        assert!(shortcode_string_needs_quoting(">closing"));
        assert!(shortcode_string_needs_quoting("a>b"));
        assert!(shortcode_string_needs_quoting("{a}"));
        assert!(shortcode_string_needs_quoting("a\"b"));
        assert!(shortcode_string_needs_quoting("a'b"));
        assert!(shortcode_string_needs_quoting("a=b"));
    }

    #[test]
    fn number_strings_must_be_quoted() {
        // would otherwise re-parse as ShortcodeArg::Number
        assert!(shortcode_string_needs_quoting("0"));
        assert!(shortcode_string_needs_quoting("800"));
        assert!(shortcode_string_needs_quoting("-1"));
        assert!(shortcode_string_needs_quoting("3.14"));
        assert!(shortcode_string_needs_quoting("1e10"));
        assert!(shortcode_string_needs_quoting("-2.5e-3"));
    }

    #[test]
    fn near_numbers_are_naked() {
        // Strings that resemble numbers but don't match the regex
        // can stay naked.
        assert!(!shortcode_string_looks_like_number("01")); // leading zero with more digits
        assert!(!shortcode_string_looks_like_number("1.")); // dot but no fraction
        assert!(!shortcode_string_looks_like_number("1e")); // exponent without digits
        assert!(!shortcode_string_looks_like_number(""));
        assert!(!shortcode_string_looks_like_number("abc"));
        assert!(!shortcode_string_looks_like_number("-"));
    }
}
fn write_insert(
    insert: &crate::pandoc::Insert,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "[++ ")?;
    for inline in &insert.content {
        write_inline(inline, buf, ctx)?;
    }
    write!(buf, "]")?;
    if !is_empty_attr(&insert.attr) {
        write_attr(&insert.attr, buf, ctx)?;
    }
    Ok(())
}
fn write_delete(
    delete: &crate::pandoc::Delete,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "[-- ")?;
    for inline in &delete.content {
        write_inline(inline, buf, ctx)?;
    }
    write!(buf, "]")?;
    if !is_empty_attr(&delete.attr) {
        write_attr(&delete.attr, buf, ctx)?;
    }
    Ok(())
}
fn write_highlight(
    highlight: &crate::pandoc::Highlight,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "[!! ")?;
    for inline in &highlight.content {
        write_inline(inline, buf, ctx)?;
    }
    write!(buf, "]")?;
    if !is_empty_attr(&highlight.attr) {
        write_attr(&highlight.attr, buf, ctx)?;
    }
    Ok(())
}
fn write_editcomment(
    comment: &crate::pandoc::EditComment,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write!(buf, "[>> ")?;
    for inline in &comment.content {
        write_inline(inline, buf, ctx)?;
    }
    write!(buf, "]")?;
    if !is_empty_attr(&comment.attr) {
        write_attr(&comment.attr, buf, ctx)?;
    }
    Ok(())
}

fn write_inline(
    inline: &crate::pandoc::Inline,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    // Before dispatch: most inline kinds open with a non-alphanumeric byte
    // (delimiter, bracket, backtick, ...), so any inlines nested inside
    // them begin with a non-alphanumeric byte-stream context. Reset
    // `prev_emitted_alnum` accordingly. `Str` is excluded because
    // `write_str` reads `prev_emitted_alnum` as set by the *preceding*
    // inline, not by an opener. `Custom` is excluded because it emits no
    // bytes and must preserve the prior state.
    if !matches!(
        inline,
        crate::pandoc::Inline::Str(_) | crate::pandoc::Inline::Custom(_)
    ) {
        ctx.prev_emitted_alnum = false;
    }

    let result = match inline {
        crate::pandoc::Inline::EditComment(node) => write_editcomment(node, buf, ctx),
        crate::pandoc::Inline::Highlight(node) => write_highlight(node, buf, ctx),
        crate::pandoc::Inline::Delete(node) => write_delete(node, buf, ctx),
        crate::pandoc::Inline::Insert(node) => write_insert(node, buf, ctx),
        crate::pandoc::Inline::Shortcode(node) => write_shortcode(node, buf),
        crate::pandoc::Inline::Attr(node) => write_attr(&node.attr, buf, ctx),
        crate::pandoc::Inline::NoteReference(node) => write_notereference(node, buf, ctx),
        crate::pandoc::Inline::Note(node) => write_note(node, buf, ctx),
        crate::pandoc::Inline::RawInline(node) => write_rawinline(node, buf, ctx),
        crate::pandoc::Inline::Cite(node) => write_cite(node, buf, ctx),
        crate::pandoc::Inline::SmallCaps(node) => write_smallcaps(node, buf, ctx),
        crate::pandoc::Inline::Underline(node) => write_underline(node, buf, ctx),
        crate::pandoc::Inline::Span(node) => write_span(node, buf, ctx),
        crate::pandoc::Inline::Quoted(node) => write_quoted(node, buf, ctx),
        crate::pandoc::Inline::Math(node) => write_math(node, buf, ctx),
        crate::pandoc::Inline::Subscript(node) => write_subscript(node, buf, ctx),
        crate::pandoc::Inline::Superscript(node) => write_superscript(node, buf, ctx),
        crate::pandoc::Inline::Strikeout(node) => write_strikeout(node, buf, ctx),
        crate::pandoc::Inline::Str(node) => write_str(node, buf, ctx),
        crate::pandoc::Inline::Space(node) => write_space(node, buf, ctx),
        crate::pandoc::Inline::SoftBreak(node) => write_soft_break(node, buf, ctx),
        crate::pandoc::Inline::Emph(node) => write_emph(node, buf, ctx),
        crate::pandoc::Inline::Strong(node) => write_strong(node, buf, ctx),
        crate::pandoc::Inline::Code(node) => write_code(node, buf, ctx),
        crate::pandoc::Inline::LineBreak(node) => write_linebreak(node, buf, ctx),
        crate::pandoc::Inline::Link(node) => write_link(node, buf, ctx),
        crate::pandoc::Inline::Image(node) => write_image(node, buf, ctx),
        crate::pandoc::Inline::Custom(_) => {
            // Custom inline nodes are not rendered in QMD output
            Ok(())
        }
    };

    // After dispatch: refresh `prev_emitted_alnum` to reflect the byte
    // this inline most recently emitted.
    match inline {
        crate::pandoc::Inline::Str(s) => {
            ctx.prev_emitted_alnum = s.text.chars().last().is_some_and(|c| c.is_alphanumeric());
        }
        crate::pandoc::Inline::Custom(_) => {
            // No bytes emitted; preserve prior state.
        }
        // Every other inline kind closes with a non-alphanumeric byte
        // (delimiter, bracket, backtick, newline, space, ...).
        _ => ctx.prev_emitted_alnum = false,
    }

    result
}

fn write_block(
    block: &crate::pandoc::Block,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    // Every block starts on its own line, so the reader's byte-stream
    // context resets here too. Clear any alphanumeric carry-over from
    // the previous block so a `Str`-leading `'` at the head of this
    // block is escaped correctly.
    ctx.prev_emitted_alnum = false;

    match block {
        Block::Plain(plain) => {
            write_plain(plain, buf, ctx)?;
        }
        Block::Paragraph(para) => {
            write_paragraph(para, buf, ctx)?;
        }
        Block::BlockQuote(blockquote) => {
            write_blockquote(blockquote, buf, ctx)?;
        }
        Block::BulletList(bulletlist) => {
            write_bulletlist(bulletlist, buf, ctx)?;
        }
        Block::OrderedList(orderedlist) => {
            write_orderedlist(orderedlist, buf, ctx)?;
        }
        Block::Div(div) => {
            write_div(div, buf, ctx)?;
        }
        Block::Header(header) => {
            write_header(header, buf, ctx)?;
        }
        Block::Table(table) => {
            write_table(table, buf, ctx)?;
        }
        Block::CodeBlock(codeblock) => {
            write_codeblock(codeblock, buf, ctx)?;
        }
        Block::LineBlock(lineblock) => {
            write_lineblock(lineblock, buf, ctx)?;
        }
        Block::RawBlock(rawblock) => {
            write_rawblock(rawblock, buf)?;
        }
        Block::DefinitionList(deflist) => {
            write_definitionlist(deflist, buf, ctx)?;
        }
        Block::HorizontalRule(rule) => {
            write_horizontalrule(rule, buf)?;
        }
        Block::Figure(figure) => {
            write_figure(figure, buf, ctx)?;
        }
        Block::BlockMetadata(metablock) => {
            write_metablock(metablock, buf, ctx)?;
        }
        Block::NoteDefinitionPara(refdef) => {
            write_inlinerefdef(refdef, buf, ctx)?;
        }
        Block::NoteDefinitionFencedBlock(refdef) => {
            write_fenced_note_definition(refdef, buf, ctx)?;
        }
        Block::CaptionBlock(_) => {
            // Defensive error: CaptionBlock should be processed during postprocessing
            ctx.errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Caption block not supported",
                )
                .with_code("Q-3-21")
                .problem("Standalone caption block cannot be rendered in QMD format")
                .add_detail(
                    "Caption blocks should be attached to figures or tables during postprocessing. \
                     This may indicate a postprocessing issue or filter-generated orphaned caption.",
                )
                .add_hint("Check for bugs in postprocessing or filters producing orphaned captions")
                .build(),
            );
        }
        Block::Custom(_) => {
            // Custom block nodes are not rendered in QMD output
        }
    }
    Ok(())
}

/// Write a sequence of prose inlines (paragraph or plain content), guarding
/// each output line against being misread as a thematic break. The content is
/// split into line segments at soft/hard breaks; any segment that would
/// canonicalize to an all-dash line (`line_is_dash_only_hazard`) is written
/// with `suppress_dash_canonicalization` set so its dashes are backslash-escaped
/// (see `write_str`). The break inlines themselves are written between segments.
fn write_prose_inlines(
    content: &[Inline],
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    let mut segment_start = 0;
    for (i, inline) in content.iter().enumerate() {
        if matches!(inline, Inline::SoftBreak(_) | Inline::LineBreak(_)) {
            write_prose_line(&content[segment_start..i], buf, ctx)?;
            write_inline(inline, buf, ctx)?;
            segment_start = i + 1;
        }
    }
    write_prose_line(&content[segment_start..], buf, ctx)?;
    Ok(())
}

fn write_prose_line(
    line: &[Inline],
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    let hazard = line_is_dash_only_hazard(line);
    if hazard {
        ctx.suppress_dash_canonicalization = true;
    }
    for inline in line {
        write_inline(inline, buf, ctx)?;
    }
    ctx.suppress_dash_canonicalization = false;
    Ok(())
}

pub fn write_paragraph(
    para: &Paragraph,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write_prose_inlines(&para.content, buf, ctx)?;
    writeln!(buf)?;
    Ok(())
}

pub fn write_plain(
    plain: &Plain,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    write_prose_inlines(&plain.content, buf, ctx)?;
    writeln!(buf)?;
    Ok(())
}

/// Write a single block to the given buffer using a fresh writer context.
///
/// This is a public wrapper around the private `write_block` function,
/// intended for use by the incremental writer (writers::incremental) which
/// needs to rewrite individual blocks.
///
/// The output ends with `\n` (each block produces its content + trailing newline).
pub fn write_single_block<T: std::io::Write>(
    block: &Block,
    buf: &mut T,
) -> Result<(), Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut ctx = QmdWriterContext::new();
    if let Err(e) = write_block(block, buf, &mut ctx) {
        return Err(vec![
            quarto_error_reporting::DiagnosticMessageBuilder::error("IO error during write")
                .with_code("Q-3-1")
                .problem(format!("Failed to write block: {}", e))
                .build(),
        ]);
    }
    if !ctx.errors.is_empty() {
        return Err(ctx.errors);
    }
    Ok(())
}

/// Write a single inline to the given buffer, using a fresh context.
///
/// This writes the inline **without** any indentation context (no BlockQuoteContext,
/// BulletListContext, etc.). This means the output is only correct for inlines
/// whose subtree does not contain SoftBreak or LineBreak — otherwise the `\n`
/// in the output would be missing indentation prefixes.
///
/// Used by the incremental writer's inline splicing (Phase 5) to produce
/// replacement text for individual inline patches.
pub fn write_single_inline<T: std::io::Write>(
    inline: &crate::pandoc::Inline,
    buf: &mut T,
) -> Result<(), Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut ctx = QmdWriterContext::new();
    if let Err(e) = write_inline(inline, buf, &mut ctx) {
        return Err(vec![
            quarto_error_reporting::DiagnosticMessageBuilder::error("IO error during write")
                .with_code("Q-3-1")
                .problem(format!("Failed to write inline: {}", e))
                .build(),
        ]);
    }
    if !ctx.errors.is_empty() {
        return Err(ctx.errors);
    }
    Ok(())
}

/// Write a sequence of inlines to the given buffer using a single fresh
/// context, with no surrounding block markers and no trailing newline.
///
/// Like [`write_single_inline`] but for a whole inline run, sharing one
/// [`QmdWriterContext`] across the sequence so emphasis-delimiter choices stay
/// consistent. Used to render metadata prose (e.g. a parsed `title`) back to
/// markdown for `q2 get-config` (bd-xoaic, GH #256). The same
/// SoftBreak/LineBreak indentation caveat as [`write_single_inline`] applies.
pub fn write_inlines<T: std::io::Write>(
    inlines: &[crate::pandoc::Inline],
    buf: &mut T,
) -> Result<(), Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut ctx = QmdWriterContext::new();
    for inline in inlines {
        if let Err(e) = write_inline(inline, buf, &mut ctx) {
            return Err(vec![
                quarto_error_reporting::DiagnosticMessageBuilder::error("IO error during write")
                    .with_code("Q-3-1")
                    .problem(format!("Failed to write inline: {}", e))
                    .build(),
            ]);
        }
    }
    if !ctx.errors.is_empty() {
        return Err(ctx.errors);
    }
    Ok(())
}

/// Write metadata (YAML front matter) to the given buffer.
///
/// This is a public wrapper around `write_config_value_meta`, intended for use
/// by the incremental writer when metadata has changed and needs to be rewritten.
///
/// Returns `Ok(true)` if metadata was written, `Ok(false)` if metadata was empty.
pub fn write_metadata<T: std::io::Write>(
    meta: &ConfigValue,
    buf: &mut T,
) -> Result<(), Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut ctx = QmdWriterContext::new();
    if let Err(e) = write_config_value_meta(meta, buf, &mut ctx) {
        return Err(vec![
            quarto_error_reporting::DiagnosticMessageBuilder::error("IO error during write")
                .with_code("Q-3-1")
                .problem(format!("Failed to write metadata: {}", e))
                .build(),
        ]);
    }
    if !ctx.errors.is_empty() {
        return Err(ctx.errors);
    }
    Ok(())
}

pub fn write<T: std::io::Write>(
    pandoc: &Pandoc,
    buf: &mut T,
) -> Result<(), Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut ctx = QmdWriterContext::new();

    // Try to write - IO errors are fatal
    if let Err(e) = write_impl(pandoc, buf, &mut ctx) {
        // IO error - wrap and return
        return Err(vec![
            quarto_error_reporting::DiagnosticMessageBuilder::error("IO error during write")
                .with_code("Q-3-1")
                .problem(format!("Failed to write QMD output: {}", e))
                .build(),
        ]);
    }

    // Check for accumulated feature errors
    if !ctx.errors.is_empty() {
        return Err(ctx.errors);
    }

    Ok(())
}

fn write_impl<T: std::io::Write>(
    pandoc: &Pandoc,
    buf: &mut T,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    // Phase 5: Write ConfigValue directly without MetaValueWithSourceInfo conversion
    let mut need_newline = write_config_value_meta(&pandoc.meta, buf, ctx)?;
    for block in &pandoc.blocks {
        if need_newline {
            writeln!(buf)?
        };
        write_block(block, buf, ctx)?;
        need_newline = true;
    }
    Ok(())
}

/// Serialize a Pandoc AST to QMD bytes and return source provenance.
///
/// The returned `SourceInfo::Concat` maps byte ranges in the output to the
/// `source_info` of the AST nodes that produced them. The pieces tile the
/// entire output with no gaps: YAML frontmatter is one piece, and each
/// top-level block (including its preceding blank-line separator) is one piece.
///
/// Use `source_info.map_offset(byte_offset, &source_context)` to resolve a
/// position in the serialized text back to the original source file and line.
pub fn write_with_source_info(
    pandoc: &Pandoc,
) -> Result<(Vec<u8>, quarto_source_map::SourceInfo), Vec<quarto_error_reporting::DiagnosticMessage>>
{
    let mut ctx = QmdWriterContext::new();
    let mut buf = Vec::new();

    let source_info = match write_impl_tracked(pandoc, &mut buf, &mut ctx) {
        Ok(si) => si,
        Err(e) => {
            return Err(vec![
                quarto_error_reporting::DiagnosticMessageBuilder::error("IO error during write")
                    .with_code("Q-3-1")
                    .problem(format!("Failed to write QMD output: {}", e))
                    .build(),
            ]);
        }
    };

    if !ctx.errors.is_empty() {
        return Err(ctx.errors);
    }

    Ok((buf, source_info))
}

/// Like `write_impl` but tracks which AST node produced each byte range.
fn write_impl_tracked(
    pandoc: &Pandoc,
    buf: &mut Vec<u8>,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<quarto_source_map::SourceInfo> {
    let mut pieces: Vec<(quarto_source_map::SourceInfo, usize)> = Vec::new();

    // Track YAML frontmatter as a single piece
    let meta_start = buf.len();
    let mut need_newline = write_config_value_meta(&pandoc.meta, buf, ctx)?;
    let meta_len = buf.len() - meta_start;
    if meta_len > 0 {
        pieces.push((pandoc.meta.source_info.clone(), meta_len));
    }

    // Track each block — include preceding blank line in measurement
    for block in &pandoc.blocks {
        let start = buf.len();
        if need_newline {
            writeln!(buf)?;
        }
        write_block(block, buf, ctx)?;
        pieces.push((block.source_info().clone(), buf.len() - start));
        need_newline = true;
    }

    Ok(quarto_source_map::SourceInfo::concat(pieces))
}
