/*
 * qmd.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::attr::is_empty_attr;
use crate::pandoc::block::MetaBlock;
use crate::pandoc::list::{ListNumberDelim, ListNumberStyle};
use crate::pandoc::meta::MetaValue;
use crate::pandoc::table::{Alignment, Cell, Table};
use crate::pandoc::{
    Block, BlockQuote, BulletList, CodeBlock, DefinitionList, Figure, Header, HorizontalRule,
    LineBlock, Meta, OrderedList, Pandoc, Paragraph, Plain, RawBlock, Str,
};
use crate::utils::string_write_adapter::StringWriteAdapter;
use hashlink::LinkedHashMap;
use std::io::{self, Write};
use yaml_rust2::{Yaml, YamlEmitter};

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

/// Convert a MetaValue to a yaml_rust2::Yaml value
/// MetaInlines and MetaBlocks are rendered using the qmd writer
fn meta_value_to_yaml(value: &MetaValue) -> std::io::Result<Yaml> {
    match value {
        MetaValue::MetaString(s) => Ok(Yaml::String(s.clone())),
        MetaValue::MetaBool(b) => Ok(Yaml::Boolean(*b)),
        MetaValue::MetaInlines(inlines) => {
            // Render inlines using the qmd writer
            let mut buffer = String::new();
            let mut adapter = StringWriteAdapter::new(&mut buffer);
            for inline in inlines {
                write_inline(inline, &mut adapter)?;
            }
            Ok(Yaml::String(buffer))
        }
        MetaValue::MetaBlocks(blocks) => {
            // Render blocks using the qmd writer
            let mut buffer = String::new();
            let mut adapter = StringWriteAdapter::new(&mut buffer);
            for (i, block) in blocks.iter().enumerate() {
                if i > 0 {
                    writeln!(&mut adapter)?;
                }
                write_block(block, &mut adapter)?;
            }
            // Trim trailing newline to avoid extra spacing in YAML
            let trimmed = buffer.trim_end();
            Ok(Yaml::String(trimmed.to_string()))
        }
        MetaValue::MetaList(list) => {
            let mut yaml_list = Vec::new();
            for item in list {
                yaml_list.push(meta_value_to_yaml(item)?);
            }
            Ok(Yaml::Array(yaml_list))
        }
        MetaValue::MetaMap(map) => {
            // LinkedHashMap preserves insertion order
            let mut yaml_map = LinkedHashMap::new();
            for (key, val) in map {
                yaml_map.insert(Yaml::String(key.clone()), meta_value_to_yaml(val)?);
            }
            Ok(Yaml::Hash(yaml_map))
        }
    }
}

fn write_meta<T: std::io::Write + ?Sized>(meta: &Meta, buf: &mut T) -> std::io::Result<bool> {
    if meta.is_empty() {
        Ok(false)
    } else {
        // Convert Meta to YAML
        // LinkedHashMap preserves insertion order
        let mut yaml_map = LinkedHashMap::new();
        for (key, value) in meta {
            yaml_map.insert(Yaml::String(key.clone()), meta_value_to_yaml(value)?);
        }
        let yaml = Yaml::Hash(yaml_map);

        // Emit YAML to string
        let mut yaml_str = String::new();
        let mut emitter = YamlEmitter::new(&mut yaml_str);
        emitter
            .dump(&yaml)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

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

fn escape_quotes(s: &str) -> String {
    s.replace("\\", "\\\\").replace('"', "\\\"")
}

fn write_attr<W: std::io::Write + ?Sized>(
    attr: &crate::pandoc::Attr,
    writer: &mut W,
) -> std::io::Result<()> {
    let (id, classes, keyvals) = attr;
    let mut wrote_something = false;
    write!(writer, "{{")?;
    if !id.is_empty() {
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

fn write_blockquote(blockquote: &BlockQuote, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    let mut blockquote_writer = BlockQuoteContext::new(buf);
    for (i, block) in blockquote.content.iter().enumerate() {
        if i > 0 {
            // Add a blank line between blocks in the blockquote
            writeln!(&mut blockquote_writer)?;
        }
        write_block(block, &mut blockquote_writer)?;
    }
    Ok(())
}

fn write_div(div: &crate::pandoc::Div, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
    write!(writer, "::: ")?;
    write_attr(&div.attr, writer)?;
    writeln!(writer)?;

    for block in div.content.iter() {
        // Add a blank line between blocks in the blockquote
        writeln!(writer)?;
        write_block(block, writer)?;
    }
    writeln!(writer, "\n:::")?;

    Ok(())
}

fn write_bulletlist(bulletlist: &BulletList, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    // Determine if this is a tight list
    // A list is tight if the first block of all items is Plain (not Para)
    let is_tight = bulletlist
        .content
        .iter()
        .all(|item| !item.is_empty() && matches!(item[0], Block::Plain(_)));

    for (i, item) in bulletlist.content.iter().enumerate() {
        if i > 0 && !is_tight {
            // Add blank line between items in loose lists
            writeln!(buf)?;
        }
        let mut item_writer = BulletListContext::new(buf);
        for (j, block) in item.iter().enumerate() {
            if j > 0 && !is_tight {
                // Add a blank line between blocks within a list item in loose lists
                writeln!(&mut item_writer)?;
            }
            write_block(block, &mut item_writer)?;
        }
    }
    Ok(())
}

fn write_orderedlist(
    orderedlist: &OrderedList,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    let (start_num, number_style, delimiter) = &orderedlist.attr;

    // Determine if this is a tight list
    // A list is tight if the first block of all items is Plain (not Para)
    let is_tight = orderedlist
        .content
        .iter()
        .all(|item| !item.is_empty() && matches!(item[0], Block::Plain(_)));

    for (i, item) in orderedlist.content.iter().enumerate() {
        if i > 0 && !is_tight {
            // Add blank line between items in loose lists
            writeln!(buf)?;
        }
        let current_num = start_num + i;
        let mut item_writer =
            OrderedListContext::new(buf, current_num, number_style.clone(), delimiter.clone());
        for (j, block) in item.iter().enumerate() {
            if j > 0 && !is_tight {
                // Add a blank line between blocks within a list item in loose lists
                writeln!(&mut item_writer)?;
            }
            write_block(block, &mut item_writer)?;
        }
    }
    Ok(())
}

fn write_header(header: &Header, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    // Write the appropriate number of # symbols for the heading level
    for _ in 0..header.level {
        write!(buf, "#")?;
    }
    write!(buf, " ")?;

    // Write the header content
    for inline in &header.content {
        write_inline(inline, buf)?;
    }

    // Add attributes if they exist
    if !is_empty_attr(&header.attr) {
        write!(buf, " ")?;
        write_attr(&header.attr, buf)?;
    }

    writeln!(buf)?;
    Ok(())
}

// FIXME this is wrong because pipe tables are quite limited (cannot have newlines in them)
fn write_cell_content(cell: &Cell, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    for (i, block) in cell.content.iter().enumerate() {
        if i > 0 {
            write!(buf, " ")?; // Join multiple blocks with space
        }
        write_block(block, buf)?;
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

fn write_codeblock(codeblock: &CodeBlock, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
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
    if !classes.is_empty() {
        // First class is typically the language
        write!(buf, "{}", classes[0])?;
        // Additional classes and attributes could be added here
    }
    if !id.is_empty() || classes.len() > 1 || !keyvals.is_empty() {
        // If there are additional attributes, write them
        write!(buf, " ")?;
        write_attr(&codeblock.attr, buf)?;
    }

    writeln!(buf)?;

    // Write the code content
    write!(buf, "{}", codeblock.text)?;

    // Ensure we end on a newline
    if !codeblock.text.ends_with('\n') {
        writeln!(buf)?;
    }

    // Write closing fence (always use backticks)
    for _ in 0..fence_length {
        write!(buf, "`")?;
    }
    writeln!(buf)?;

    Ok(())
}

fn write_lineblock(lineblock: &LineBlock, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    for (i, line) in lineblock.content.iter().enumerate() {
        if i > 0 {
            writeln!(buf)?;
        }
        write!(buf, "| ")?;
        for inline in line {
            write_inline(inline, buf)?;
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
        // For other formats, use fenced raw block notation
        writeln!(buf, "```{{{}}}", rawblock.format)?;
        write!(buf, "{}", rawblock.text)?;
        if !rawblock.text.ends_with('\n') {
            writeln!(buf)?;
        }
        writeln!(buf, "```")?;
    }
    Ok(())
}

fn write_definitionlist(
    deflist: &DefinitionList,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    for (i, (term, definitions)) in deflist.content.iter().enumerate() {
        if i > 0 {
            writeln!(buf)?;
        }

        // Write the term
        for inline in term {
            write_inline(inline, buf)?;
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
                write_block(block, buf)?;
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

fn write_figure(figure: &Figure, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    // Write figure using div syntax with fig- class
    write!(buf, "::: ")?;
    write_attr(&figure.attr, buf)?;
    writeln!(buf)?;

    // Write the figure content
    for block in &figure.content {
        writeln!(buf)?;
        write_block(block, buf)?;
    }

    // Write caption if it exists
    if let Some(ref long_caption) = figure.caption.long {
        if !long_caption.is_empty() {
            writeln!(buf)?;
            for (i, block) in long_caption.iter().enumerate() {
                if i > 0 {
                    writeln!(buf)?;
                }
                write_block(block, buf)?;
            }
        }
    } else if let Some(ref short_caption) = figure.caption.short {
        if !short_caption.is_empty() {
            writeln!(buf)?;
            // Convert short caption (inlines) to a paragraph for consistency
            for inline in short_caption {
                write_inline(inline, buf)?;
            }
            writeln!(buf)?;
        }
    }

    writeln!(buf, "\n:::")?;
    Ok(())
}

fn write_metablock(metablock: &MetaBlock, buf: &mut dyn std::io::Write) -> std::io::Result<bool> {
    write_meta(&metablock.meta, buf)
}

fn write_inlinerefdef(
    refdef: &crate::pandoc::block::NoteDefinitionPara,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write!(buf, "[^{}]: ", refdef.id)?;
    for inline in &refdef.content {
        write_inline(inline, buf)?;
    }
    writeln!(buf)?;
    Ok(())
}

fn write_fenced_note_definition(
    refdef: &crate::pandoc::block::NoteDefinitionFencedBlock,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    writeln!(buf, "::: ^{}", refdef.id)?;
    for (i, block) in refdef.content.iter().enumerate() {
        if i > 0 {
            // Add a blank line between blocks
            writeln!(buf)?;
        }
        write_block(block, buf)?;
    }
    writeln!(buf, ":::")?;
    Ok(())
}

fn write_table(table: &Table, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
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
            let mut content = String::new();
            let mut adapter = StringWriteAdapter::new(&mut content);
            write_cell_content(cell, &mut adapter)?;
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
    if let Some(ref long_caption) = table.caption.long {
        if !long_caption.is_empty() {
            writeln!(buf)?; // Blank line before caption
            for block in long_caption {
                // Extract inline content from Plain blocks in caption
                if let Block::Plain(plain) = block {
                    write!(buf, ": ")?;
                    for inline in &plain.content {
                        write_inline(inline, buf)?;
                    }
                    writeln!(buf)?;
                }
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

// Helper function to reverse smart quotes conversion
// Converts Unicode right single quotation mark (') back to ASCII apostrophe (')
fn reverse_smart_quotes(text: &str) -> String {
    text.replace('\u{2019}', "'")
}

// Helper function to escape special markdown characters
// This follows Pandoc's escaping rules for text strings
fn escape_markdown(text: &str) -> String {
    let mut result = String::new();
    for ch in text.chars() {
        match ch {
            // Backslash must be escaped first (conceptually)
            // But since we're processing char by char, we just escape it when we see it
            '\\' => result.push_str("\\\\"),
            // Greater-than sign must be escaped to avoid blockquote interpretation
            '>' => result.push_str("\\>"),
            // Hash must be escaped to avoid header interpretation
            '#' => result.push_str("\\#"),
            // Other characters pass through unchanged
            _ => result.push(ch),
        }
    }
    result
}

fn write_str(s: &Str, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    let text = reverse_smart_quotes(&s.text);
    let escaped = escape_markdown(&text);
    write!(buf, "{}", escaped)
}

fn write_space(_: &crate::pandoc::Space, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    write!(buf, " ")
}

fn write_soft_break(
    _: &crate::pandoc::SoftBreak,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    // Pandoc's writer for markdown outputs a space for soft breaks
    // We choose to deviate from Pandoc for roundtripping purposes
    writeln!(buf)
}

fn write_emph(emph: &crate::pandoc::Emph, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    write!(buf, "*")?;
    for inline in &emph.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "*")
}

fn write_strong(
    strong: &crate::pandoc::Strong,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write!(buf, "**")?;
    for inline in &strong.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "**")
}

fn write_code(code: &crate::pandoc::Code, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
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
    if !is_empty_attr(&code.attr) {
        write_attr(&code.attr, buf)?;
    }
    Ok(())
}

fn write_linebreak(
    _line_break: &crate::pandoc::LineBreak,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write!(buf, "\\")?;
    writeln!(buf)
}

fn write_link(link: &crate::pandoc::Link, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    write!(buf, "[")?;
    for inline in &link.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "](")?;
    write!(buf, "{}", link.target.0)?;
    if !link.target.1.is_empty() {
        write!(buf, " \"{}\"", escape_quotes(&link.target.1))?;
    }
    write!(buf, ")")?;
    if !is_empty_attr(&link.attr) {
        write_attr(&link.attr, buf)?;
    }
    Ok(())
}

fn write_image(image: &crate::pandoc::Image, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    write!(buf, "![")?;
    for inline in &image.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "](")?;
    write!(buf, "{}", image.target.0)?;
    if !image.target.1.is_empty() {
        write!(buf, " \"{}\"", escape_quotes(&image.target.1))?;
    }
    write!(buf, ")")?;
    if !is_empty_attr(&image.attr) {
        write_attr(&image.attr, buf)?;
    }
    Ok(())
}

fn write_strikeout(
    strikeout: &crate::pandoc::Strikeout,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write!(buf, "~~")?;
    for inline in &strikeout.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "~~")
}

fn write_subscript(
    subscript: &crate::pandoc::Subscript,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write!(buf, "~")?;
    for inline in &subscript.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "~")
}

fn write_superscript(
    superscript: &crate::pandoc::Superscript,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write!(buf, "^")?;
    for inline in &superscript.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "^")
}

fn write_math(math: &crate::pandoc::Math, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
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
) -> std::io::Result<()> {
    match quoted.quote_type {
        crate::pandoc::QuoteType::SingleQuote => {
            write!(buf, "'")?;
            for inline in &quoted.content {
                write_inline(inline, buf)?;
            }
            write!(buf, "'")?;
        }
        crate::pandoc::QuoteType::DoubleQuote => {
            write!(buf, "\"")?;
            for inline in &quoted.content {
                write_inline(inline, buf)?;
            }
            write!(buf, "\"")?;
        }
    }
    Ok(())
}
fn write_span(span: &crate::pandoc::Span, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
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
                write_inline(inline, buf)?;
            }
            write!(buf, "]")?;
            return Ok(());
        }
    }

    // Spans always use bracket syntax: [content]{#id .class key=value}
    // Even empty attributes should be written as [content]{} for proper roundtripping
    write!(buf, "[")?;
    for inline in &span.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "]")?;
    write_attr(&span.attr, buf)?;
    Ok(())
}

fn write_underline(
    underline: &crate::pandoc::Underline,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write!(buf, "[")?;
    for inline in &underline.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "]{{.underline}}")
}
fn write_smallcaps(
    smallcaps: &crate::pandoc::SmallCaps,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write!(buf, "[")?;
    for inline in &smallcaps.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "]{{.smallcaps}}")
}
fn write_cite(cite: &crate::pandoc::Cite, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
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
                write_inline(inline, buf)?;
            }
            // Only add space if prefix exists and doesn't end with whitespace
            if has_prefix {
                let ends_with_space = citation
                    .prefix
                    .last()
                    .map(|inline| matches!(inline, crate::pandoc::Inline::Space(_)))
                    .unwrap_or(false);
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
                write_inline(inline, buf)?;
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
                    write_inline(inline, buf)?;
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
) -> std::io::Result<()> {
    // Only output raw content if it's for markdown format
    if raw.format == "markdown" {
        write!(buf, "{}", raw.text)
    } else {
        // For other formats, use raw span notation with = prefix
        write!(buf, "`{}`{{={}}}", raw.text, raw.format)
    }
}
fn write_note(note: &crate::pandoc::Note, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    write!(buf, "^[")?;
    for (i, block) in note.content.iter().enumerate() {
        if i > 0 {
            write!(buf, " ")?;
        }
        // For inline notes, we need to flatten block content
        match block {
            crate::pandoc::Block::Plain(plain) => {
                for inline in &plain.content {
                    write_inline(inline, buf)?;
                }
            }
            crate::pandoc::Block::Paragraph(para) => {
                for inline in &para.content {
                    write_inline(inline, buf)?;
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
) -> std::io::Result<()> {
    write!(buf, "[^{}]", noteref.id)
}
fn write_shortcode(
    shortcode: &crate::pandoc::Shortcode,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write!(buf, "{{{{{}}}}}", shortcode.name)
}
fn write_insert(
    insert: &crate::pandoc::Insert,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write!(buf, "[++ ")?;
    for inline in &insert.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "]")?;
    if !is_empty_attr(&insert.attr) {
        write_attr(&insert.attr, buf)?;
    }
    Ok(())
}
fn write_delete(
    delete: &crate::pandoc::Delete,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write!(buf, "[-- ")?;
    for inline in &delete.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "]")?;
    if !is_empty_attr(&delete.attr) {
        write_attr(&delete.attr, buf)?;
    }
    Ok(())
}
fn write_highlight(
    highlight: &crate::pandoc::Highlight,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write!(buf, "[!! ")?;
    for inline in &highlight.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "]")?;
    if !is_empty_attr(&highlight.attr) {
        write_attr(&highlight.attr, buf)?;
    }
    Ok(())
}
fn write_editcomment(
    comment: &crate::pandoc::EditComment,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    write!(buf, "[>> ")?;
    for inline in &comment.content {
        write_inline(inline, buf)?;
    }
    write!(buf, "]")?;
    if !is_empty_attr(&comment.attr) {
        write_attr(&comment.attr, buf)?;
    }
    Ok(())
}

fn write_inline(
    inline: &crate::pandoc::Inline,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    match inline {
        crate::pandoc::Inline::EditComment(node) => write_editcomment(node, buf),
        crate::pandoc::Inline::Highlight(node) => write_highlight(node, buf),
        crate::pandoc::Inline::Delete(node) => write_delete(node, buf),
        crate::pandoc::Inline::Insert(node) => write_insert(node, buf),
        crate::pandoc::Inline::Shortcode(node) => write_shortcode(node, buf),
        crate::pandoc::Inline::Attr(node) => write_attr(node, buf),
        crate::pandoc::Inline::NoteReference(node) => write_notereference(node, buf),
        crate::pandoc::Inline::Note(node) => write_note(node, buf),
        crate::pandoc::Inline::RawInline(node) => write_rawinline(node, buf),
        crate::pandoc::Inline::Cite(node) => write_cite(node, buf),
        crate::pandoc::Inline::SmallCaps(node) => write_smallcaps(node, buf),
        crate::pandoc::Inline::Underline(node) => write_underline(node, buf),
        crate::pandoc::Inline::Span(node) => write_span(node, buf),
        crate::pandoc::Inline::Quoted(node) => write_quoted(node, buf),
        crate::pandoc::Inline::Math(node) => write_math(node, buf),
        crate::pandoc::Inline::Subscript(node) => write_subscript(node, buf),
        crate::pandoc::Inline::Superscript(node) => write_superscript(node, buf),
        crate::pandoc::Inline::Strikeout(node) => write_strikeout(node, buf),
        crate::pandoc::Inline::Str(node) => write_str(node, buf),
        crate::pandoc::Inline::Space(node) => write_space(node, buf),
        crate::pandoc::Inline::SoftBreak(node) => write_soft_break(node, buf),
        crate::pandoc::Inline::Emph(node) => write_emph(node, buf),
        crate::pandoc::Inline::Strong(node) => write_strong(node, buf),
        crate::pandoc::Inline::Code(node) => write_code(node, buf),
        crate::pandoc::Inline::LineBreak(node) => write_linebreak(node, buf),
        crate::pandoc::Inline::Link(node) => write_link(node, buf),
        crate::pandoc::Inline::Image(node) => write_image(node, buf),
    }
}

fn write_block(block: &crate::pandoc::Block, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    match block {
        Block::Plain(plain) => {
            write_plain(plain, buf)?;
        }
        Block::Paragraph(para) => {
            write_paragraph(para, buf)?;
        }
        Block::BlockQuote(blockquote) => {
            write_blockquote(blockquote, buf)?;
        }
        Block::BulletList(bulletlist) => {
            write_bulletlist(bulletlist, buf)?;
        }
        Block::OrderedList(orderedlist) => {
            write_orderedlist(orderedlist, buf)?;
        }
        Block::Div(div) => {
            write_div(div, buf)?;
        }
        Block::Header(header) => {
            write_header(header, buf)?;
        }
        Block::Table(table) => {
            write_table(table, buf)?;
        }
        Block::CodeBlock(codeblock) => {
            write_codeblock(codeblock, buf)?;
        }
        Block::LineBlock(lineblock) => {
            write_lineblock(lineblock, buf)?;
        }
        Block::RawBlock(rawblock) => {
            write_rawblock(rawblock, buf)?;
        }
        Block::DefinitionList(deflist) => {
            write_definitionlist(deflist, buf)?;
        }
        Block::HorizontalRule(rule) => {
            write_horizontalrule(rule, buf)?;
        }
        Block::Figure(figure) => {
            write_figure(figure, buf)?;
        }
        Block::BlockMetadata(metablock) => {
            write_metablock(metablock, buf)?;
        }
        Block::NoteDefinitionPara(refdef) => {
            write_inlinerefdef(refdef, buf)?;
        }
        Block::NoteDefinitionFencedBlock(refdef) => {
            write_fenced_note_definition(refdef, buf)?;
        }
        Block::CaptionBlock(_) => {
            panic!(
                "CaptionBlock found in QMD writer - should have been processed during postprocessing"
            )
        }
    }
    Ok(())
}

pub fn write_paragraph(para: &Paragraph, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    for inline in &para.content {
        write_inline(inline, buf)?;
    }
    writeln!(buf)?;
    Ok(())
}

pub fn write_plain(plain: &Plain, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    for inline in &plain.content {
        write_inline(inline, buf)?;
    }
    writeln!(buf)?;
    Ok(())
}

pub fn write<T: std::io::Write>(pandoc: &Pandoc, buf: &mut T) -> std::io::Result<()> {
    let mut need_newline = write_meta(&pandoc.meta, buf)?;
    for block in &pandoc.blocks {
        if need_newline {
            write!(buf, "\n")?
        };
        write_block(block, buf)?;
        need_newline = true;
    }
    Ok(())
}
