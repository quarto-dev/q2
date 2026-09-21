/*
 * convert.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The entry point format writers call: TeX in, target markup out.
//!
//! This is the seam pampa's docx and Typst writers use. It does not depend
//! on pampa's AST types: the caller passes the math text, whether it was
//! display math, and the text's `SourceInfo` (pampa's `Math.text_source`,
//! falling back to the node's `source_info`), and gets back either the
//! target markup or, when the expression could not be fully converted, no
//! markup and the diagnostics explaining why. Warnings can accompany
//! markup; errors never do (half-converted math is worse than verbatim
//! TeX, which the caller emits instead, in a code style).

use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceInfo;

use crate::ast::Node;
use crate::diagnostics::diagnostics;
use crate::normalize::{Mode, Normalized, normalize};
use crate::spec::Spec;
use crate::{omml, typst};

/// Output language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Office Math Markup Language, embeddable (no namespace declarations).
    Omml,
    /// Plain Typst math, the inside of a `$ … $` block.
    Typst,
}

/// Result of [`convert`].
#[derive(Debug, Clone)]
pub struct Conversion {
    /// Target markup, or `None` when the expression had errors and the
    /// caller must fall back to the verbatim TeX.
    pub output: Option<String>,
    /// Everything worth reporting, located inside the math text's source.
    pub diagnostics: Vec<DiagnosticMessage>,
    /// The normalized tree, for callers that want to inspect it.
    pub tree: Normalized,
}

impl Conversion {
    /// Whether the conversion produced markup.
    pub fn succeeded(&self) -> bool {
        self.output.is_some()
    }
}

/// Convert `text` for `target`.
pub fn convert(
    text: &str,
    mode: Mode,
    target: Target,
    text_source: &SourceInfo,
    spec: &Spec,
) -> Conversion {
    let tree = normalize(text, mode, spec);
    let diagnostics = diagnostics(&tree, text_source);
    let output = if tree.root.has_errors() {
        None
    } else {
        Some(render(&tree, target))
    };
    Conversion {
        output,
        diagnostics,
        tree,
    }
}

/// Render an already-normalized tree, regardless of errors (error nodes
/// become literal runs / strings). Useful for previews and tests.
pub fn render(tree: &Normalized, target: Target) -> String {
    match target {
        Target::Omml => omml::to_omml(tree),
        Target::Typst => typst::to_typst(tree),
    }
}

/// Convenience for callers that only have a tree.
pub fn has_errors(root: &Node) -> bool {
    root.has_errors()
}
