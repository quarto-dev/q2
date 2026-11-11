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

#[cfg(test)]
mod tests {
    use super::*;

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
