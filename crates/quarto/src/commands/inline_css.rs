//! `q2 call inline-css` — hidden helper for the Pandoc filter chain.
//!
//! Reads an HTML fragment on stdin and writes it to stdout with the rules of
//! any `<style>` blocks moved onto the matching elements' `style` attributes
//! (see [`quarto_core::inline_css`]). `normalize/astpipeline.lua` still pipes
//! raw HTML tables through this for typst; the `css-inline` pipeline stage
//! has normally inlined them already by then.

use std::io::{Read, Write};

use anyhow::Result;

pub fn execute() -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let output = quarto_core::inline_css::inline_css(&input)?;
    std::io::stdout().write_all(output.as_bytes())?;
    Ok(())
}
