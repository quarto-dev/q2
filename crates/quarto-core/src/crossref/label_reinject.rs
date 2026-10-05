/*
 * crossref/label_reinject.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Re-inject a crossref-consumed `label:` cell option into the text
//! handed to the knitr engine (bd-2lxj10z0, plan
//! `claude-notes/plans/2026-09-28-knitr-label-preengine-sugaring-fix.md`).
//!
//! [`super::codeblock_shorthand`] lifts a crossref-classified `label:`
//! out of a code cell's body and onto the wrapping [`Block::Div`]'s `id`,
//! for q2's own crossref numbering (`::: {#fig-cars}`). knitr never sees
//! that `label:` again once the AST is serialized, so it falls back to a
//! positional name (`unnamed-chunk-N`) instead of the label-derived one
//! (`fig-cars-1.svg`) it would otherwise produce.
//!
//! This is deliberately additive at serialization time, not a change to
//! the stored AST (plan §D2): [`inject`] is meant to run on a **clone**
//! made just for the engine about to execute (`EngineExecutionStage`'s
//! `masked_ast`), immediately before `serialize_ast_to_qmd`. The AST that
//! `quarto_ast_reconcile::reconcile` compares against is never touched,
//! and the wrapping Div keeps its `id` — it still needs the label
//! independently for crossref numbering.

use quarto_pandoc_types::block::{Block, CodeBlock};
use quarto_pandoc_types::pandoc::Pandoc;

use super::RefTypeRegistry;

/// Walk `doc`, re-injecting `#| label: <id>` (in each cell's own comment
/// syntax) into any code block directly wrapped by a Div whose `id`
/// classifies as a crossref label — the exact shape
/// [`super::codeblock_shorthand::try_desugar_code_block`]'s `Float`
/// wrapper produces.
///
/// `registry` is `None` on pipeline paths that never ran
/// `PreEngineSugaringStage` (e.g. some replay entry points). Those paths
/// never produced a label-consuming wrapper Div in the first place, so
/// skipping is a no-op, not a missed fix.
pub fn inject(doc: &mut Pandoc, registry: Option<&RefTypeRegistry>) {
    let Some(registry) = registry else {
        return;
    };
    for_each_wrapped_cell(&mut doc.blocks, registry, &mut inject_label);
}

/// The exact inverse of [`inject`]: remove the `#| label: <id>` line it
/// prepends, from any code block directly wrapped by a crossref-labelled
/// Div whose `id` is `<id>`.
///
/// Used by the capture splice (bd-mu9i0bct). A recorded
/// `EngineCapture::input_qmd` is the text knitr *received*, so its cells
/// still carry the re-injected line; the live pre-engine AST never does.
/// Splice keys cells by a hash of their text, so the capture's AST must be
/// brought back to the live shape before keying. Only the exact line
/// `inject` writes is removed — a differently-valued or hand-written
/// `label:` is left alone — and the same registry gate as `inject` applies,
/// so an unclassified Div (which `inject` never touched) is skipped too.
/// Captures with no injected line (other engines, older recordings) are a
/// no-op.
pub fn strip(doc: &mut Pandoc, registry: Option<&RefTypeRegistry>) {
    let Some(registry) = registry else {
        return;
    };
    for_each_wrapped_cell(&mut doc.blocks, registry, &mut strip_label);
}

/// Recurse over exactly the containers
/// [`super::codeblock_shorthand::desugar_blocks`] recurses over, so this
/// walk visits every position a wrapper Div could have been produced at,
/// calling `f(cell, div_id)` for each code block directly inside a Div whose
/// `id` classifies as a crossref label.
fn for_each_wrapped_cell(
    blocks: &mut [Block],
    registry: &RefTypeRegistry,
    f: &mut impl FnMut(&mut CodeBlock, &str),
) {
    for block in blocks {
        match block {
            Block::Div(div) => {
                if registry.classify_cite_id(&div.attr.0).is_some() {
                    for child in &mut div.content {
                        if let Block::CodeBlock(cb) = child {
                            f(cb, &div.attr.0);
                        }
                    }
                }
                for_each_wrapped_cell(&mut div.content, registry, f);
            }
            Block::BlockQuote(b) => for_each_wrapped_cell(&mut b.content, registry, f),
            Block::OrderedList(b) => {
                for item in &mut b.content {
                    for_each_wrapped_cell(item, registry, f);
                }
            }
            Block::BulletList(b) => {
                for item in &mut b.content {
                    for_each_wrapped_cell(item, registry, f);
                }
            }
            Block::DefinitionList(b) => {
                for (_term, defs) in &mut b.content {
                    for item in defs {
                        for_each_wrapped_cell(item, registry, f);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The line [`inject_label`] prepends, newline included.
fn injected_line(cb: &CodeBlock, id: &str) -> String {
    let language = super::codeblock_shorthand::language_of(cb);
    let syntax = crate::cell_options::comment_syntax_for(&language);
    let suffix = syntax.suffix.unwrap_or("");
    format!("{}| label: {id}{suffix}\n", syntax.prefix)
}

/// Remove exactly the line [`inject_label`] would prepend, if `cb.text`
/// starts with it.
fn strip_label(cb: &mut CodeBlock, id: &str) {
    let line = injected_line(cb, id);
    if let Some(rest) = cb.text.strip_prefix(&line) {
        cb.text = rest.to_string();
    }
}

/// Prepend `#| label: <id>` (in `cb`'s own comment syntax) to `cb.text`,
/// unless the leading option-line run already has a `label:` key. By
/// construction `try_desugar_code_block`'s `Float` wrapper only exists
/// because it just consumed (removed) exactly that key, so the guard
/// should never trigger in practice — it is cheap insurance against
/// double-injection, not a load-bearing check.
fn inject_label(cb: &mut CodeBlock, id: &str) {
    let language = super::codeblock_shorthand::language_of(cb);
    let syntax = crate::cell_options::comment_syntax_for(&language);
    if has_label_option(&cb.text, &syntax) {
        return;
    }
    cb.text = format!("{}{}", injected_line(cb, id), cb.text);
}

/// True iff `text`'s leading run of option lines already has a `label:`
/// key. Mirrors the option-line shape `crate::cell_options` parses
/// closely enough for this guard — a whole-string scan, not a real
/// parse, is enough since this only ever exists to avoid a double-insert.
fn has_label_option(text: &str, syntax: &crate::cell_options::CommentSyntax) -> bool {
    for line in text.lines() {
        let Some(rest) = line.strip_prefix(syntax.prefix) else {
            break;
        };
        let rest = rest.trim_start_matches([' ', '\t']);
        let Some(rest) = rest.strip_prefix('|') else {
            break;
        };
        let content = match syntax.suffix {
            Some(suffix) => rest.trim_end().strip_suffix(suffix).unwrap_or(rest),
            None => rest,
        };
        if let Some((key, _)) = content.split_once(':')
            && key.trim() == "label"
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::RefTypeRegistry;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::block::Div;
    use quarto_source_map::{FileId, SourceInfo};

    fn registry() -> RefTypeRegistry {
        RefTypeRegistry::builtin()
    }

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn code_block(text: &str) -> CodeBlock {
        CodeBlock {
            attr: (String::new(), vec!["r".to_string()], LinkedHashMap::new()),
            text: text.to_string(),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        }
    }

    fn wrapped(id: &str, cb: CodeBlock) -> Pandoc {
        Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Div(Div {
                attr: (id.to_string(), Vec::new(), LinkedHashMap::new()),
                content: vec![Block::CodeBlock(cb)],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            })],
        }
    }

    #[test]
    fn injects_label_into_wrapped_code_block() {
        let mut doc = wrapped("fig-cars", code_block("plot(cars)\n"));
        inject(&mut doc, Some(&registry()));
        let Block::Div(div) = &doc.blocks[0] else {
            panic!("expected Div");
        };
        let Block::CodeBlock(cb) = &div.content[0] else {
            panic!("expected CodeBlock");
        };
        assert_eq!(cb.text, "#| label: fig-cars\nplot(cars)\n");
        // The Div's own id is untouched — crossref numbering still needs it.
        assert_eq!(div.attr.0, "fig-cars");
    }

    #[test]
    fn skips_when_id_is_not_a_crossref_label() {
        let mut doc = wrapped("not-a-reftype-prefix", code_block("plot(cars)\n"));
        inject(&mut doc, Some(&registry()));
        let Block::Div(div) = &doc.blocks[0] else {
            panic!("expected Div");
        };
        let Block::CodeBlock(cb) = &div.content[0] else {
            panic!("expected CodeBlock");
        };
        assert_eq!(cb.text, "plot(cars)\n");
    }

    #[test]
    fn skips_when_registry_is_none() {
        let mut doc = wrapped("fig-cars", code_block("plot(cars)\n"));
        inject(&mut doc, None);
        let Block::Div(div) = &doc.blocks[0] else {
            panic!("expected Div");
        };
        let Block::CodeBlock(cb) = &div.content[0] else {
            panic!("expected CodeBlock");
        };
        assert_eq!(cb.text, "plot(cars)\n");
    }

    #[test]
    fn does_not_double_inject_when_label_already_present() {
        let mut doc = wrapped("fig-cars", code_block("#| label: fig-cars\nplot(cars)\n"));
        inject(&mut doc, Some(&registry()));
        let Block::Div(div) = &doc.blocks[0] else {
            panic!("expected Div");
        };
        let Block::CodeBlock(cb) = &div.content[0] else {
            panic!("expected CodeBlock");
        };
        assert_eq!(cb.text, "#| label: fig-cars\nplot(cars)\n");
    }

    #[test]
    fn strip_is_the_inverse_of_inject() {
        let mut doc = wrapped("fig-cars", code_block("plot(cars)\n"));
        inject(&mut doc, Some(&registry()));
        strip(&mut doc, Some(&registry()));
        let Block::Div(div) = &doc.blocks[0] else {
            panic!("expected Div");
        };
        let Block::CodeBlock(cb) = &div.content[0] else {
            panic!("expected CodeBlock");
        };
        assert_eq!(cb.text, "plot(cars)\n");
    }

    #[test]
    fn strip_leaves_a_different_label_alone() {
        let mut doc = wrapped("fig-cars", code_block("#| label: fig-other\nplot(cars)\n"));
        strip(&mut doc, Some(&registry()));
        let Block::Div(div) = &doc.blocks[0] else {
            panic!("expected Div");
        };
        let Block::CodeBlock(cb) = &div.content[0] else {
            panic!("expected CodeBlock");
        };
        assert_eq!(cb.text, "#| label: fig-other\nplot(cars)\n");
    }

    #[test]
    fn strip_skips_unclassified_div_and_none_registry() {
        let text = "#| label: not-a-reftype-prefix\nplot(cars)\n";
        let mut doc = wrapped("not-a-reftype-prefix", code_block(text));
        strip(&mut doc, Some(&registry()));
        strip(&mut doc, None);
        let Block::Div(div) = &doc.blocks[0] else {
            panic!("expected Div");
        };
        let Block::CodeBlock(cb) = &div.content[0] else {
            panic!("expected CodeBlock");
        };
        assert_eq!(cb.text, text);
    }
}
