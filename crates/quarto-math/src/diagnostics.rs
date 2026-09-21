/*
 * diagnostics.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! From normalization [`Problem`]s to `quarto-error-reporting` diagnostics.
//!
//! Each [`ProblemKind`] owns one `Q-22-*` code (subsystem `math`; the
//! catalog entries, `docs/errors/math/` pages and sidebar rows land with
//! the code). A problem's span is a byte range in the math *text*; the
//! caller supplies the text's `SourceInfo` (pampa's `Math.text_source`)
//! and the diagnostic location becomes a `substring` of it, so it resolves
//! to the exact `.qmd` characters even across folded lines and stripped
//! block-quote gutters.

use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::SourceInfo;

use crate::normalize::{Normalized, Problem, ProblemKind};

/// The `Q-22-*` code for a problem kind.
pub fn code_for(kind: ProblemKind) -> &'static str {
    match kind {
        ProblemKind::UnknownCommand => "Q-22-1",
        ProblemKind::UnknownEnvironment => "Q-22-2",
        ProblemKind::EnvironmentMismatch => "Q-22-3",
        ProblemKind::ArityMismatch => "Q-22-4",
        ProblemKind::UnbalancedBrace => "Q-22-5",
        ProblemKind::UnbalancedDelimiter => "Q-22-6",
        ProblemKind::DoubleScript => "Q-22-7",
        ProblemKind::ExpansionLimit => "Q-22-8",
        ProblemKind::Unsupported => "Q-22-9",
        ProblemKind::RaggedRows => "Q-22-10",
        ProblemKind::IgnoredLimits => "Q-22-11",
        ProblemKind::Dropped => "Q-22-12",
        ProblemKind::Syntax => "Q-22-13",
    }
}

/// The catalog title for a problem kind (kept identical to the catalog;
/// the diagnostics test checks this).
pub fn title_for(kind: ProblemKind) -> &'static str {
    match kind {
        ProblemKind::UnknownCommand => "Unknown Math Command",
        ProblemKind::UnknownEnvironment => "Unknown Math Environment",
        ProblemKind::EnvironmentMismatch => "Mismatched Math Environment",
        ProblemKind::ArityMismatch => "Math Command Missing An Argument",
        ProblemKind::UnbalancedBrace => "Unbalanced Brace In Math",
        ProblemKind::UnbalancedDelimiter => "Unbalanced Left And Right Delimiters",
        ProblemKind::DoubleScript => "Double Subscript Or Superscript",
        ProblemKind::ExpansionLimit => "Math Macro Expansion Limit Exceeded",
        ProblemKind::Unsupported => "Unsupported Math Command",
        ProblemKind::RaggedRows => "Ragged Rows In Math Environment",
        ProblemKind::IgnoredLimits => "Limits Command Ignored",
        ProblemKind::Dropped => "Math Construct Dropped",
        ProblemKind::Syntax => "Math Syntax Error",
    }
}

/// Errors stop the conversion (the seam falls back to verbatim TeX);
/// warnings describe output that was produced with a caveat.
pub fn severity_for(kind: ProblemKind) -> DiagnosticKind {
    match kind {
        ProblemKind::Unsupported
        | ProblemKind::RaggedRows
        | ProblemKind::IgnoredLimits
        | ProblemKind::Dropped => DiagnosticKind::Warning,
        _ => DiagnosticKind::Error,
    }
}

fn hint_for(kind: ProblemKind) -> Option<&'static str> {
    Some(match kind {
        ProblemKind::UnknownCommand => {
            "Check the spelling, or define the command in the expression with \\newcommand."
        }
        ProblemKind::UnknownEnvironment => {
            "Supported environments include aligned, cases, pmatrix, bmatrix, array and gathered."
        }
        ProblemKind::EnvironmentMismatch => {
            "Every \\begin{name} needs a matching \\end{name} in the same expression."
        }
        ProblemKind::ArityMismatch => "Supply every argument in braces.",
        ProblemKind::UnbalancedBrace => "Balance the braces; write \\{ and \\} for literal braces.",
        ProblemKind::UnbalancedDelimiter => {
            "Pair every \\left with a \\right; use \\right. for an invisible delimiter."
        }
        ProblemKind::DoubleScript => "Group the intended structure with braces, e.g. x^{2^3}.",
        ProblemKind::ExpansionLimit => "A macro probably refers to itself; remove the recursion.",
        ProblemKind::RaggedRows => "Give every row the same number of & separated cells.",
        ProblemKind::IgnoredLimits => "Place \\limits right after a big operator such as \\sum.",
        ProblemKind::Unsupported | ProblemKind::Dropped | ProblemKind::Syntax => return None,
    })
}

/// Build one diagnostic for a problem, located inside `text_source`.
pub fn diagnostic_for(problem: &Problem, text_source: &SourceInfo) -> DiagnosticMessage {
    let kind = problem.kind;
    let location = match &problem.span {
        Some(r) => SourceInfo::substring(text_source.clone(), r.start, r.end),
        None => text_source.clone(),
    };
    let mut builder = match severity_for(kind) {
        DiagnosticKind::Warning => DiagnosticMessageBuilder::warning(title_for(kind)),
        DiagnosticKind::Info => DiagnosticMessageBuilder::info(title_for(kind)),
        _ => DiagnosticMessageBuilder::error(title_for(kind)),
    }
    .with_code(code_for(kind))
    .problem(problem.message.clone())
    .with_location(location);
    if let Some(hint) = hint_for(kind) {
        builder = builder.add_hint(hint);
    }
    builder.build()
}

/// Diagnostics for every problem of a normalized expression.
pub fn diagnostics(n: &Normalized, text_source: &SourceInfo) -> Vec<DiagnosticMessage> {
    n.problems
        .iter()
        .map(|p| diagnostic_for(p, text_source))
        .collect()
}
