/*
 * qmd.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::{Block, BlockQuote, Meta, Pandoc, Paragraph};
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

fn write_meta<T: std::io::Write>(_meta: &Meta, buf: &mut T) -> std::io::Result<()> {
    writeln!(buf, "---")?;
    writeln!(buf, "unfinished: true")?;
    writeln!(buf, "---")?;
    Ok(())
}

fn escape_quotes(s: &str) -> String {
    s.replace("\\", "\\\\").replace('"', "\\\"")
}

fn write_attr(attr: &crate::pandoc::Attr, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
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

fn write_blockquote<T: std::io::Write + ?Sized>(
    blockquote: &BlockQuote,
    buf: &mut T,
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

fn write_block(block: &crate::pandoc::Block, buf: &mut dyn std::io::Write) -> std::io::Result<()> {
    match block {
        Block::Paragraph(para) => {
            write_paragraph(para, buf)?;
        }
        Block::BlockQuote(blockquote) => {
            write_blockquote(blockquote, buf)?;
        }
        Block::Div(div) => {
            write_div(div, buf)?;
        }
        _ => todo!(),
    }
    Ok(())
}

pub fn write_paragraph<T: std::io::Write + ?Sized>(
    para: &Paragraph,
    buf: &mut T,
) -> std::io::Result<()> {
    for inline in &para.content {
        match inline {
            crate::pandoc::Inline::Str(s) => {
                // FIXME what are the escaping rules that Pandoc uses?
                write!(buf, "{}", s.text)?;
            }
            crate::pandoc::Inline::Space(_) => {
                write!(buf, " ")?;
            }
            _ => todo!(),
        }
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
