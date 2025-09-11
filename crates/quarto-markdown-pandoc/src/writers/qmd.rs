/*
 * qmd.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::{Block, BlockQuote, Meta, Pandoc, Paragraph, Space, Str};

pub fn write_meta<T: std::io::Write>(_meta: &Meta, buf: &mut T) -> std::io::Result<()> {
    writeln!(buf, "---")?;
    writeln!(buf, "unfinished: true")?;
    writeln!(buf, "---")?;
    Ok(())
}

pub fn write_blockquote<T: std::io::Write>(
    blockquote: &BlockQuote,
    buf: &mut T,
) -> std::io::Result<()> {
    // this implementation is incorrect!!
    writeln!(buf, "> ")?;
    for block in &blockquote.content {
        write_block(block, buf)?;
    }
    Ok(())
}

pub fn write_paragraph<T: std::io::Write>(para: &Paragraph, buf: &mut T) -> std::io::Result<()> {
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

pub fn write_block<T: std::io::Write>(
    block: &crate::pandoc::Block,
    buf: &mut T,
) -> std::io::Result<()> {
    match block {
        Block::Paragraph(para) => {
            write_paragraph(para, buf)?;
        }
        Block::BlockQuote(blockquote) => {
            write_blockquote(blockquote, buf)?;
        }
        _ => todo!(),
    }
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
