//! KNR-1 Phase 1A/1D: the three-level comparison of two markdown strings, and the class it
//! gives them. No R needed.
//!
//! Levels (S6 decision): bytes; the parsed AST (`pampa::readers::qmd::read`, printed with
//! `writers::native::write`, which carries no source locations); and the rendered HTML (the
//! whole pipeline after the engine, run on the markdown through a fixed-output engine).

use std::sync::Arc;

use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError,
};
use quarto_core::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::NativeRuntime;

/// How two documents relate, weakest difference first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    /// Same bytes.
    Identical,
    /// Different bytes, same parsed AST (blank lines, fence spelling).
    SyntaxLevel,
    /// Different AST, same rendered HTML.
    Semantic,
    /// Different rendered HTML.
    Blocker,
}

impl Class {
    pub fn label(self) -> &'static str {
        match self {
            Class::Identical => "identical",
            Class::SyntaxLevel => "syntax-level",
            Class::Semantic => "semantic",
            Class::Blocker => "blocker",
        }
    }
}

/// The parsed AST printed without source locations; `Err` carries a parse failure.
pub fn native_ast(markdown: &str) -> Result<String, String> {
    let (pandoc, ctx, _diagnostics) = pampa::readers::qmd::read(
        markdown.as_bytes(),
        false,
        "corpus.md",
        &mut std::io::sink(),
        false,
        None,
    )
    .map_err(|e| format!("{e:?}"))?;
    let mut out = Vec::new();
    pampa::writers::native::write(&pandoc, &ctx, &mut out).map_err(|e| format!("{e:?}"))?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// An engine named `knitr` that claims `r` and returns a fixed markdown string, so the
/// pipeline after the engine runs on exactly that markdown (S6 level 3).
struct FixedMarkdownEngine(String);

impl ExecutionEngine for FixedMarkdownEngine {
    fn name(&self) -> &str {
        "knitr"
    }
    fn execute(
        &self,
        _input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        Ok(ExecuteResult::new(self.0.clone()))
    }
    fn claims_language(
        &self,
        language: &str,
        _first_class: Option<&str>,
    ) -> quarto_core::engine::LanguageClaim {
        if language == "r" {
            quarto_core::engine::LanguageClaim::Primary(1)
        } else {
            quarto_core::engine::LanguageClaim::None
        }
    }
    fn try_claims_language(
        &self,
        language: &str,
        first_class: Option<&str>,
    ) -> Option<quarto_core::engine::LanguageClaim> {
        Some(self.claims_language(language, first_class))
    }
    fn is_available(&self) -> bool {
        true
    }
}

/// Render `markdown` as the engine output of a one-chunk document, to HTML.
/// The output file's name is replaced so two renders compare equal.
pub fn render_html(markdown: &str) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let input = dir.path().join("doc.qmd");
    std::fs::write(&input, "---\ntitle: corpus\n---\n\n```{r}\n1\n```\n")
        .map_err(|e| e.to_string())?;
    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(FixedMarkdownEngine(markdown.to_string())));
    let options = RenderToFileOptions {
        engine_registry_override: Some(Arc::new(registry)),
        quiet: true,
        ..Default::default()
    };
    let result = render_to_file(&input, "html", &options, Arc::new(NativeRuntime::new()))
        .map_err(|e| format!("{e:?}"))?;
    std::fs::read_to_string(&result.output_path).map_err(|e| e.to_string())
}

/// Classify `candidate` against `oracle`. A markdown that does not parse or render counts as
/// a difference at that level (a blocker when the rendering fails on one side only).
pub fn classify(oracle: &str, candidate: &str) -> Class {
    classify_with(oracle, candidate, &render_html)
}

pub fn classify_with(
    oracle: &str,
    candidate: &str,
    html: &dyn Fn(&str) -> Result<String, String>,
) -> Class {
    if oracle == candidate {
        return Class::Identical;
    }
    match (native_ast(oracle), native_ast(candidate)) {
        (Ok(a), Ok(b)) if a == b => return Class::SyntaxLevel,
        _ => {}
    }
    match (html(oracle), html(candidate)) {
        (Ok(a), Ok(b)) if a == b => Class::Semantic,
        _ => Class::Blocker,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_identical_bytes() {
        assert_eq!(classify("a\n\nb\n", "a\n\nb\n"), Class::Identical);
    }

    #[test]
    fn classify_syntax_level_when_ast_equal() {
        assert_eq!(classify("a\n\nb\n", "a\n\n\n\nb\n"), Class::SyntaxLevel);
    }

    #[test]
    fn classify_semantic_when_html_equal() {
        // A raw `<span>` and a bracketed span differ in the AST and render alike.
        let oracle = "<span class=\"x\">t</span>\n";
        let candidate = "[t]{.x}\n";
        assert_ne!(native_ast(oracle), native_ast(candidate));
        assert_eq!(classify(oracle, candidate), Class::Semantic);
    }

    #[test]
    fn classify_blocker_when_html_differs() {
        assert_eq!(classify("a\n", "b\n"), Class::Blocker);
    }

    #[test]
    fn classes_order_by_severity() {
        assert!(Class::Identical < Class::SyntaxLevel);
        assert!(Class::Semantic < Class::Blocker);
    }
}
