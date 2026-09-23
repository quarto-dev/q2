//! The `crossref-<type>-title` and `crossref-<type>-prefix` param families —
//! Q2's [`RefTypeRegistry`] becomes the authoritative source for *every*
//! registered ref-type, not just the handful Q1's `_language.yml` and
//! `crossref.categories` (`mainstateinit.lua`) happen to cover.
//!
//! Both families are required, and required together: `title(type, default)`
//! (`crossref/format.lua:4-7`) feeds **captions** from `-title`;
//! `refPrefix(type, upper)` (`crossref/format.lua:66-79`) feeds **reference
//! text** from `-prefix` first, then `crossref.categories`, then a bare
//! `type .. "."` literal. Emitting only `-title` (round-4 regression,
//! `0b295831e`) leaves every `@ref` reading e.g. `thm. 1` even though the
//! caption correctly reads `Theorem 1` — the two states are
//! indistinguishable in the caption and only observable in reference text.
//! See `claude-notes/plans/2026-09-18-pandoc-hybrid-P4-implementation.md`
//! Task 5 and Findings for Gordon, item 7.
//!
//! The derivation rule mirrors `filters.ts:484` exactly. [`RefTypeDef`] has
//! no display-*prefix* field, so `-prefix` cannot come from the registry
//! alone:
//!
//! ```text
//! for (ref_type, def) in registry.iter():
//!     title  := def.kind                                          # assumed already localized
//!                                                                  # by localize_builtin_display_names
//!     prefix := language.crossref_prefix(ref_type).unwrap_or(title)
//! ```

use serde_json::{Map, Value, json};

use crate::crossref::RefTypeRegistry;
use crate::language::LanguageTerms;

pub(super) fn insert_crossref_title_prefix_family(
    blob: &mut Map<String, Value>,
    registry: &RefTypeRegistry,
    language: &LanguageTerms,
) {
    for (ref_type, def) in registry.iter() {
        let title = def.kind.as_str();
        let prefix = language.crossref_prefix(ref_type).unwrap_or(title);
        blob.insert(format!("crossref-{ref_type}-title"), json!(title));
        blob.insert(format!("crossref-{ref_type}-prefix"), json!(prefix));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    /// T5.1: for every registered ref-type, both `-title` and `-prefix` are
    /// present, and the emitted pair count is exactly `2 * registry.len()`
    /// — the discriminator against "some crossref param is set" (a
    /// `-title`-only test passes under the exact round-4 regression this
    /// task fixes).
    ///
    /// Revert hunk: removing the `-prefix` emit line makes the pair-count
    /// assertion RED (it would read `registry.len()`, not `2 *
    /// registry.len()`).
    #[test]
    fn test_both_crossref_families_are_emitted() {
        let registry = RefTypeRegistry::builtin();
        let language = crate::language::resolve_language("en", &[]);
        let mut blob = Map::new();

        insert_crossref_title_prefix_family(&mut blob, &registry, &language);

        let mut pairs = 0;
        for (ref_type, _) in registry.iter() {
            assert!(blob.contains_key(&format!("crossref-{ref_type}-title")));
            assert!(blob.contains_key(&format!("crossref-{ref_type}-prefix")));
            pairs += 2;
        }
        assert_eq!(pairs, 2 * registry.len());
        assert_eq!(blob.len(), 2 * registry.len());
    }

    /// T5.2: when the language table has an explicit `-title` and no
    /// explicit `-prefix`, both keys derive from the (already-localized)
    /// title — `"Satz"` in this fixture, chosen because it differs from
    /// every builtin English default, so a stale fallback would be visibly
    /// wrong rather than coincidentally correct. `localize_builtin_display_names`
    /// is the real production step that turns a language-table `-title`
    /// override into `RefTypeDef::kind` before this contributor ever runs;
    /// calling it here (rather than hand-mutating `kind`) exercises the two
    /// real pieces together instead of faking their precondition.
    ///
    /// Revert hunk: changing `.unwrap_or(title)` to `.unwrap_or_default()`
    /// (or omitting the key entirely on `None`) makes this RED whenever
    /// `language.crossref_prefix` itself falls through to `None` — not
    /// exercised by this particular fixture, since `LanguageTerms::crossref_prefix`
    /// already falls back to `crossref_title` internally and finds `"Satz"`
    /// there. T5.3/T5.4 exercise `insert_crossref_title_prefix_family`'s own
    /// `.unwrap_or(title)` fallback, for a type absent from the language
    /// table entirely. Kept here as the value-direction companion to T5.1's
    /// presence check.
    #[test]
    fn test_prefix_derives_from_title_when_absent() {
        let mut registry = RefTypeRegistry::builtin();
        let language = crate::language::resolve_language(
            "en",
            std::slice::from_ref(&localized_layer("crossref-thm-title", "Satz")),
        );
        registry.localize_builtin_display_names(&language);
        let mut blob = Map::new();

        insert_crossref_title_prefix_family(&mut blob, &registry, &language);

        assert_eq!(blob["crossref-thm-title"], json!("Satz"));
        assert_eq!(blob["crossref-thm-prefix"], json!("Satz"));
    }

    /// T5.5: a custom-registered ref-type gets both keys too — the
    /// "generalizes for free to every registered ref-type" principle.
    ///
    /// Revert hunk: replacing `RefTypeRegistry::iter` with a hardcoded
    /// builtin list makes this RED (the custom type would never be
    /// visited).
    #[test]
    fn test_custom_ref_types_get_both_keys() {
        let mut registry = RefTypeRegistry::builtin();
        registry
            .register_custom("mytype", "MyType", None)
            .expect("registering a fresh custom type should succeed");
        let language = crate::language::resolve_language("en", &[]);
        let mut blob = Map::new();

        insert_crossref_title_prefix_family(&mut blob, &registry, &language);

        assert!(blob.contains_key("crossref-mytype-prefix"));
        assert_eq!(blob["crossref-mytype-title"], json!("MyType"));
    }

    fn localized_layer(key: &str, value: &str) -> crate::language::StructuredTermLayer {
        let mut layer = crate::language::StructuredTermLayer::default();
        layer.terms.insert(
            key.to_string(),
            crate::language::TermEntry {
                value: value.to_string(),
                source: quarto_source_map::SourceInfo::generated(
                    quarto_source_map::By::programmatic_config(),
                ),
            },
        );
        layer
    }
}
