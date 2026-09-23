//! The `MetaBlocks`→`MetaInlines` coercion for `title`/`subtitle` (P7 Task
//! 6; the docx-relevant half of Task 8's Bug A).
//!
//! A multi-line YAML scalar (e.g. `title: |\n  My Title`) parses as
//! [`quarto_pandoc_types::config_value::ConfigValueKind::PandocBlocks`] (a
//! single `Paragraph`), which the shared JSON writer
//! (`pampa::writers::json`) serializes as Pandoc's `MetaBlocks`. Pandoc's
//! docx/pptx writers do not read `MetaBlocks` for `title` the way they read
//! `MetaInlines` — **(measured)** with a normal inline title, pandoc 3.11
//! puts it in `docProps/core.xml` as `<dc:title>My Title</dc:title>` and in
//! `word/document.xml` as a `w:pStyle w:val="Title"` paragraph; a
//! `MetaBlocks`-valued title does not reliably reach either surface the
//! same way.
//!
//! Q1's own `ensureMetaInlines` (Lua,
//! `resources/filters/normalize/normalize.lua`) applies this coercion to
//! `subtitle` only — Q1's TypeScript front-matter processing already
//! normalizes `title` to inlines earlier, upstream of the point that Lua
//! filter runs. Q2's Pandoc-hybrid leg has no equivalent earlier
//! normalization, so both `title` and `subtitle` need the coercion here.
//!
//! Deliberately scoped to these two keys, not a whole-`Meta` generic
//! walk: `author`/`date` are unaffected by this bug (they map to
//! `MetaInlines`/`MetaList` shapes some other way already — see Task 6's
//! T6.1), and a blanket coercion would also flatten fields that are
//! legitimately block-valued (e.g. `abstract`).

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_pandoc_types::inline::{Inline, SoftBreak};

/// The metadata keys this coercion applies to.
pub const META_INLINE_COERCE_KEYS: &[&str] = &["title", "subtitle"];

/// Coerce `title`/`subtitle` from `PandocBlocks` to `PandocInlines` in
/// place, if present and block-valued. A no-op for any other kind
/// (already-inline, absent, or a non-Pandoc scalar/map).
pub fn coerce_meta_blocks_to_inlines(meta: &mut ConfigValue) {
    for key in META_INLINE_COERCE_KEYS {
        let Some(entry) = meta.get_mut(key) else {
            continue;
        };
        if let ConfigValueKind::PandocBlocks(blocks) = &entry.value {
            let inlines = blocks_to_inlines(blocks);
            entry.value = ConfigValueKind::PandocInlines(inlines);
        }
    }
}

/// Flatten a block sequence into an inline sequence by taking each
/// `Plain`/`Paragraph`'s own inline content, joined by a `SoftBreak`
/// between blocks. Other block kinds (list, table, …) are dropped —
/// outside this coercion's scope, since a title/subtitle is a single
/// short YAML scalar in every case observed in practice.
fn blocks_to_inlines(blocks: &[Block]) -> Vec<Inline> {
    let mut out = Vec::new();
    for block in blocks {
        let content: &[Inline] = match block {
            Block::Plain(p) => &p.content,
            Block::Paragraph(p) => &p.content,
            _ => continue,
        };
        if !out.is_empty() {
            out.push(Inline::SoftBreak(SoftBreak {
                source_info: quarto_source_map::SourceInfo::generated(
                    quarto_source_map::By::unknown(),
                ),
            }));
        }
        out.extend(content.iter().cloned());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::block::Paragraph;
    use quarto_pandoc_types::inline::Str;
    use quarto_source_map::SourceInfo;

    fn str_inline(s: &str) -> Inline {
        Inline::Str(Str {
            text: s.to_string(),
            source_info: SourceInfo::for_test(),
        })
    }

    fn meta_with(key: &str, value: ConfigValue) -> ConfigValue {
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: key.to_string(),
                key_source: SourceInfo::for_test(),
                value,
            }],
            SourceInfo::for_test(),
        )
    }

    fn block_value(text: &str) -> ConfigValue {
        let blocks = vec![Block::Paragraph(Paragraph {
            content: vec![str_inline(text)],
            source_info: SourceInfo::for_test(),
        })];
        ConfigValue {
            value: ConfigValueKind::PandocBlocks(blocks),
            source_info: SourceInfo::for_test(),
            merge_op: Default::default(),
        }
    }

    /// T6.2: a block-valued `title` becomes `MetaInlines` (here:
    /// `PandocInlines`, the Q2-internal kind the JSON writer maps to
    /// `MetaInlines`), and its flattened text equals the source text —
    /// both assertions are required (the tag alone is satisfiable by an
    /// empty-inlines coercion).
    #[test]
    fn test_block_valued_title_coerced() {
        let mut meta = meta_with("title", block_value("My Title"));
        coerce_meta_blocks_to_inlines(&mut meta);

        let entry = meta.get("title").unwrap();
        assert!(
            matches!(entry.value, ConfigValueKind::PandocInlines(_)),
            "expected PandocInlines after coercion, got {:?}",
            entry.value
        );
        assert_eq!(
            entry.as_plain_text().as_deref(),
            Some("My Title"),
            "flattened text must equal the source text"
        );
    }

    /// The same coercion applies to `subtitle`.
    #[test]
    fn test_block_valued_subtitle_coerced() {
        let mut meta = meta_with("subtitle", block_value("A Subtitle"));
        coerce_meta_blocks_to_inlines(&mut meta);

        let entry = meta.get("subtitle").unwrap();
        assert!(matches!(entry.value, ConfigValueKind::PandocInlines(_)));
        assert_eq!(entry.as_plain_text().as_deref(), Some("A Subtitle"));
    }

    /// An already-inline title is left alone (no double-coercion, no
    /// panic on a non-Blocks kind).
    #[test]
    fn test_inline_valued_title_untouched() {
        let mut meta = meta_with(
            "title",
            ConfigValue::new_inlines(vec![str_inline("Already Inline")], SourceInfo::for_test()),
        );
        coerce_meta_blocks_to_inlines(&mut meta);
        let entry = meta.get("title").unwrap();
        assert!(matches!(entry.value, ConfigValueKind::PandocInlines(_)));
        assert_eq!(entry.as_plain_text().as_deref(), Some("Already Inline"));
    }

    /// A key outside the coercion list (e.g. `author`) is never touched,
    /// even if block-valued — this coercion is deliberately narrow.
    #[test]
    fn test_other_keys_are_not_coerced() {
        let mut meta = meta_with("author", block_value("Alice"));
        coerce_meta_blocks_to_inlines(&mut meta);
        let entry = meta.get("author").unwrap();
        assert!(
            matches!(entry.value, ConfigValueKind::PandocBlocks(_)),
            "author must be left as PandocBlocks — this coercion is title/subtitle-only"
        );
    }

    /// Absent keys are a no-op, not a panic.
    #[test]
    fn test_absent_keys_are_noop() {
        let mut meta = ConfigValue::new_map(vec![], SourceInfo::for_test());
        coerce_meta_blocks_to_inlines(&mut meta);
        assert!(meta.get("title").is_none());
    }
}
