/*
 * qmd.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::{Block, BlockQuote, BulletList, CodeBlock, Header, Meta, OrderedList, Pandoc, Paragraph, Plain};
use crate::pandoc::table::{Alignment, Table, Cell};
use crate::pandoc::list::ListNumberDelim;
use crate::pandoc::attr::is_empty_attr;
use std::io::{self, Write};

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
    delimiter: ListNumberDelim,
    indent: String,
}

impl<'a, W: Write + ?Sized> OrderedListContext<'a, W> {
    fn new(inner: &'a mut W, number: usize, delimiter: ListNumberDelim) -> Self {
        // Calculate indent based on the width of the number + delimiter
        let delim_str = match delimiter {
            ListNumberDelim::Period => ".",
            ListNumberDelim::OneParen => ")",
            ListNumberDelim::TwoParens => ")",
            _ => ".",
        };
        let marker = format!("{}{} ", number, delim_str);
        let indent = " ".repeat(marker.len());
        
        Self {
            inner,
            at_line_start: true,
            is_first_line: true,
            number,
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
                    let delim_str = match self.delimiter {
                        ListNumberDelim::Period => ".",
                        ListNumberDelim::OneParen => ")",
                        ListNumberDelim::TwoParens => ")",
                        _ => ".",
                    };
                    write!(self.inner, "{}{} ", self.number, delim_str)?;
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

fn write_meta<T: std::io::Write>(_meta: &Meta, buf: &mut T) -> std::io::Result<()> {
    writeln!(buf, "---")?;
    writeln!(buf, "unfinished: true")?;
    writeln!(buf, "---")?;
    Ok(())
}

fn escape_quotes(s: &str) -> String {
    s.replace("\\", "\\\\").replace('"', "\\\"")
}

fn write_attr<W: std::io::Write + ?Sized>(attr: &crate::pandoc::Attr, writer: &mut W) -> std::io::Result<()> {
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

fn write_blockquote(
    blockquote: &BlockQuote,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
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

fn write_bulletlist(
    bulletlist: &BulletList,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    // Determine if this is a tight list
    // A list is tight if all items contain exactly one Plain block
    let is_tight = bulletlist.content.iter().all(|item| {
        item.len() == 1 && matches!(item[0], Block::Plain(_))
    });
    
    for (i, item) in bulletlist.content.iter().enumerate() {
        if i > 0 && !is_tight {
            // Add blank line between items in loose lists
            writeln!(buf)?;
        }
        let mut item_writer = BulletListContext::new(buf);
        for (j, block) in item.iter().enumerate() {
            if j > 0 {
                // Add a blank line between blocks within a list item
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
    let (start_num, _number_style, delimiter) = &orderedlist.attr;
    
    // Determine if this is a tight list
    // A list is tight if all items contain exactly one Plain block
    let is_tight = orderedlist.content.iter().all(|item| {
        item.len() == 1 && matches!(item[0], Block::Plain(_))
    });
    
    for (i, item) in orderedlist.content.iter().enumerate() {
        if i > 0 && !is_tight {
            // Add blank line between items in loose lists
            writeln!(buf)?;
        }
        let current_num = start_num + i;
        let mut item_writer = OrderedListContext::new(buf, current_num, delimiter.clone());
        for (j, block) in item.iter().enumerate() {
            if j > 0 {
                // Add a blank line between blocks within a list item
                writeln!(&mut item_writer)?;
            }
            write_block(block, &mut item_writer)?;
        }
    }
    Ok(())
}

fn write_header(
    header: &Header,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
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

fn write_cell_content(cell: &Cell, buf: &mut String) -> std::io::Result<()> {
    for (i, block) in cell.content.iter().enumerate() {
        if i > 0 {
            buf.push(' '); // Join multiple blocks with space
        }
        match block {
            Block::Plain(plain) => {
                for inline in &plain.content {
                    write_inline_to_string(inline, buf)?;
                }
            }
            Block::Paragraph(para) => {
                for inline in &para.content {
                    write_inline_to_string(inline, buf)?;
                }
            }
            _ => {
                // For complex blocks, just use a placeholder
                buf.push_str("[complex content]");
            }
        }
    }
    Ok(())
}

fn write_inline_to_string(inline: &crate::pandoc::Inline, buf: &mut String) -> std::io::Result<()> {
    match inline {
        crate::pandoc::Inline::Str(s) => {
            buf.push_str(&s.text);
        }
        crate::pandoc::Inline::Space(_) => {
            buf.push(' ');
        }
        crate::pandoc::Inline::SoftBreak(_) => {
            buf.push(' ');
        }
        crate::pandoc::Inline::Emph(emph) => {
            buf.push('*');
            for inline in &emph.content {
                write_inline_to_string(inline, buf)?;
            }
            buf.push('*');
        }
        crate::pandoc::Inline::Strong(strong) => {
            buf.push_str("**");
            for inline in &strong.content {
                write_inline_to_string(inline, buf)?;
            }
            buf.push_str("**");
        }
        crate::pandoc::Inline::Code(code) => {
            buf.push('`');
            buf.push_str(&code.text);
            buf.push('`');
        }
        _ => {
            // For other inline types, use a placeholder for now
            buf.push_str("[inline]");
        }
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
    // Determine fence type and length - use backticks unless the code contains them
    let fence_char = if codeblock.text.contains("```") { '~' } else { '`' };
    let fence_length = 3; // Start with 3, could be made smarter to handle nested fences
    
    // Write opening fence
    for _ in 0..fence_length {
        write!(buf, "{}", fence_char)?;
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
    
    // Write closing fence
    for _ in 0..fence_length {
        write!(buf, "{}", fence_char)?;
    }
    writeln!(buf)?;
    
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
            write_cell_content(cell, &mut content)?;
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

fn write_inline(
    inline: &crate::pandoc::Inline,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    match inline {
        crate::pandoc::Inline::Str(s) => {
            // FIXME what are the escaping rules that Pandoc uses?
            write!(buf, "{}", s.text)?;
        }
        crate::pandoc::Inline::Space(_) => {
            write!(buf, " ")?;
        }
        crate::pandoc::Inline::SoftBreak(_) => {
            writeln!(buf)?;
        }
        crate::pandoc::Inline::Emph(emph) => {
            write!(buf, "*")?;
            for inline in &emph.content {
                write_inline(inline, buf)?;
            }
            write!(buf, "*")?;
        }
        crate::pandoc::Inline::Strong(strong) => {
            write!(buf, "**")?;
            for inline in &strong.content {
                write_inline(inline, buf)?;
            }
            write!(buf, "**")?;
        }
        crate::pandoc::Inline::Code(code) => {
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
        }
        crate::pandoc::Inline::LineBreak(_) => {
            write!(buf, "\\")?;
            writeln!(buf)?;
        }
        crate::pandoc::Inline::Link(link) => {
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
        }
        crate::pandoc::Inline::Image(image) => {
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
        }
        crate::pandoc::Inline::Strikeout(strikeout) => {
            write!(buf, "~~")?;
            for inline in &strikeout.content {
                write_inline(inline, buf)?;
            }
            write!(buf, "~~")?;
        }
        crate::pandoc::Inline::Superscript(superscript) => {
            write!(buf, "^")?;
            for inline in &superscript.content {
                write_inline(inline, buf)?;
            }
            write!(buf, "^")?;
        }
        crate::pandoc::Inline::Subscript(subscript) => {
            write!(buf, "~")?;
            for inline in &subscript.content {
                write_inline(inline, buf)?;
            }
            write!(buf, "~")?;
        }
        crate::pandoc::Inline::Math(math) => {
            match math.math_type {
                crate::pandoc::MathType::InlineMath => {
                    write!(buf, "${}$", math.text)?;
                }
                crate::pandoc::MathType::DisplayMath => {
                    write!(buf, "$${}$$", math.text)?;
                }
            }
        }
        crate::pandoc::Inline::Quoted(quoted) => {
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
        }
        crate::pandoc::Inline::Span(span) => {
            // Spans with attributes use bracket syntax: [content]{#id .class key=value}
            if !is_empty_attr(&span.attr) {
                write!(buf, "[")?;
                for inline in &span.content {
                    write_inline(inline, buf)?;
                }
                write!(buf, "]")?;
                write_attr(&span.attr, buf)?;
            } else {
                // Spans without attributes just output their content
                for inline in &span.content {
                    write_inline(inline, buf)?;
                }
            }
        }
        inline => panic!("Unhandled inline type: {:?}", inline),
    }
    Ok(())
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
        block => panic!("Unhandled block type in write_block: {:?}", block),
    }
    Ok(())
}

pub fn write_paragraph(
    para: &Paragraph,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    for inline in &para.content {
        write_inline(inline, buf)?;
    }
    writeln!(buf)?;
    Ok(())
}

pub fn write_plain(
    plain: &Plain,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    for inline in &plain.content {
        write_inline(inline, buf)?;
    }
    writeln!(buf)?;
    Ok(())
}

pub fn write<T: std::io::Write>(pandoc: &Pandoc, buf: &mut T) -> std::io::Result<()> {
    write_meta(&pandoc.meta, buf)?;
    for block in &pandoc.blocks {
        write!(buf, "\n")?;
        write_block(block, buf)?;
    }
    Ok(())
}
