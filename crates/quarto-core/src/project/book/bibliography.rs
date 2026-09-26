/*
 * project/book/bibliography.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Project-wide bibliography merge for multi-file HTML books
 * (book-projects P6). See
 * `claude-notes/plans/2026-09-21-book-projects-P6-bibliography.md`.
 */

use std::collections::{HashMap, HashSet};
use std::path::Path;

use pampa::citeproc_filter::{ChapterCitationManifest, CiteprocConfig};
use pampa::pandoc::Block;
use quarto_citeproc::{Citation, CitationItem, Processor, Reference};

use crate::error::{QuartoError, Result};

/// Merge every chapter's citation manifest into one project-wide,
/// deduplicated citation order plus the union of cited references, in book
/// order.
///
/// Returns the cited ids in first-cited-across-the-book order
/// (deduplicated) and the full [`Reference`] for each id — the shape
/// [`build_merged_bibliography`] needs to seed one fresh `Processor` over
/// the union. When the same id is cited with slightly different `Reference`
/// data across chapters (should not happen in practice — the same
/// bibliography entry, loaded twice), the *first* chapter's copy wins,
/// mirroring the crossref registry's own first-definition-wins policy.
pub fn aggregate_chapter_citations(
    manifests: &[ChapterCitationManifest],
) -> (Vec<String>, Vec<Reference>) {
    let mut order: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut refs_by_id: HashMap<String, Reference> = HashMap::new();

    for manifest in manifests {
        for id in &manifest.cited_ids {
            if seen.insert(id.clone()) {
                order.push(id.clone());
            }
        }
        for reference in &manifest.references {
            refs_by_id
                .entry(reference.id.clone())
                .or_insert_with(|| reference.clone());
        }
    }

    let references = order
        .iter()
        .filter_map(|id| refs_by_id.get(id).cloned())
        .collect();

    (order, references)
}

/// Build the project-wide merged bibliography: one fresh
/// `quarto_citeproc::Processor` over the union of every chapter's cited
/// references, seeded with citation-number and disambiguation state in
/// book-wide first-cited order.
///
/// The seeding calls (`get_initial_citation_number`,
/// `process_citations_with_disambiguation`) are not optional polish —
/// `generate_bibliography`/`generate_bibliography_to_outputs` read both
/// pieces of state from the processor rather than computing them
/// themselves (design doc §9): skipping them would silently produce a
/// blank/unnumbered bibliography for a numeric CSL style, or wrong
/// year-suffix disambiguation for two colliding author+year references.
///
/// `config`/`base_dir` are the designated references chapter's own citeproc
/// configuration (CSL style, declaration-site base dir) — the merge reuses
/// the same style every chapter's own per-document pass already resolved,
/// rather than re-resolving it project-wide.
pub fn build_merged_bibliography(
    config: &CiteprocConfig,
    base_dir: &Path,
    cited_order: &[String],
    references: Vec<Reference>,
) -> Result<Vec<Block>> {
    let style = pampa::citeproc_filter::load_csl_style(config, base_dir).map_err(|e| {
        QuartoError::other(format!(
            "failed to load CSL style for book-wide bibliography merge: {e}"
        ))
    })?;

    let mut processor = Processor::new(style);
    processor.add_references(references);

    // Seed citation-number state in book-wide first-cited order (numeric
    // CSL styles read this at bibliography-format time).
    for id in cited_order {
        processor.get_initial_citation_number(id);
    }

    // Seed disambiguation state (year-suffix/et-al) with one synthetic
    // Citation per id, same order — the only interface
    // `process_citations_with_disambiguation` exposes for a document-free
    // caller.
    let synthetic_citations: Vec<Citation> = cited_order
        .iter()
        .map(|id| Citation {
            id: None,
            note_number: None,
            items: vec![CitationItem {
                id: id.clone(),
                ..Default::default()
            }],
        })
        .collect();
    processor
        .process_citations_with_disambiguation(&synthetic_citations)
        .map_err(|e| {
            QuartoError::other(format!(
                "citation disambiguation failed during book-wide bibliography merge: {e}"
            ))
        })?;

    pampa::citeproc_filter::generate_bibliography(&mut processor).map_err(|e| {
        QuartoError::other(format!(
            "failed to format book-wide merged bibliography: {e}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(id: &str) -> Reference {
        serde_json::from_str(&format!(r#"{{"id":"{id}","type":"book"}}"#))
            .expect("minimal CSL-JSON reference must deserialize")
    }

    fn manifest(cited_ids: &[&str]) -> ChapterCitationManifest {
        let cited_ids: Vec<String> = cited_ids.iter().map(|s| s.to_string()).collect();
        ChapterCitationManifest {
            references: cited_ids.iter().map(|id| reference(id)).collect(),
            cited_ids,
        }
    }

    #[test]
    fn duplicate_id_across_chapters_dedupes_to_one_entry() {
        let manifests = vec![manifest(&["smith2020"]), manifest(&["smith2020"])];
        let (order, references) = aggregate_chapter_citations(&manifests);
        assert_eq!(order, vec!["smith2020".to_string()]);
        assert_eq!(references.len(), 1);
    }

    #[test]
    fn id_cited_in_only_one_chapter_appears_once() {
        let manifests = vec![manifest(&["jones2019"]), manifest(&["smith2020"])];
        let (order, references) = aggregate_chapter_citations(&manifests);
        assert_eq!(
            order,
            vec!["jones2019".to_string(), "smith2020".to_string()]
        );
        assert_eq!(references.len(), 2);
    }

    #[test]
    fn book_order_is_preserved_across_chapters() {
        let manifests = vec![manifest(&["b2020", "a2019"]), manifest(&["c2021", "a2019"])];
        let (order, _) = aggregate_chapter_citations(&manifests);
        assert_eq!(
            order,
            vec![
                "b2020".to_string(),
                "a2019".to_string(),
                "c2021".to_string()
            ]
        );
    }

    #[test]
    fn chapter_citation_manifest_round_trips_through_json() {
        let m = manifest(&["a2019", "b2020"]);
        let json = serde_json::to_string(&m).expect("manifest must serialize");
        let decoded: ChapterCitationManifest =
            serde_json::from_str(&json).expect("manifest must deserialize");
        assert_eq!(decoded.cited_ids, m.cited_ids);
        assert_eq!(decoded.references.len(), m.references.len());
    }
}
