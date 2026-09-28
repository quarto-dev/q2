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
    inject_blocks(&mut doc.blocks, registry);
}

/// Recurse over exactly the containers
/// [`super::codeblock_shorthand::desugar_blocks`] recurses over, so this
/// walk visits every position a wrapper Div could have been produced at.
fn inject_blocks(blocks: &mut [Block], registry: &RefTypeRegistry) {
    for block in blocks {
        inject_block(block, registry);
    }
}

fn inject_block(block: &mut Block, registry: &RefTypeRegistry) {
    match block {
        Block::Div(div) => {
            if registry.classify_cite_id(&div.attr.0).is_some() {
                for child in &mut div.content {
                    if let Block::CodeBlock(cb) = child {
                        inject_label(cb, &div.attr.0);
                    }
                }
            }
            inject_blocks(&mut div.content, registry);
        }
        Block::BlockQuote(b) => inject_blocks(&mut b.content, registry),
        Block::OrderedList(b) => {
            for item in &mut b.content {
                inject_blocks(item, registry);
            }
        }
        Block::BulletList(b) => {
            for item in &mut b.content {
                inject_blocks(item, registry);
            }
        }
        Block::DefinitionList(b) => {
            for (_term, defs) in &mut b.content {
                for item in defs {
                    inject_blocks(item, registry);
                }
            }
        }
        _ => {}
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
    let suffix = syntax.suffix.unwrap_or("");
    cb.text = format!("{}| label: {id}{suffix}\n{}", syntax.prefix, cb.text);
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
}
