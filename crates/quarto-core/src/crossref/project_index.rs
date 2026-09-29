/*
 * crossref/project_index.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Project-wide crossref registry for multi-file books (book-projects P5).
 */

//! Project-wide crossref registry for multi-file books.
//!
//! Multi-file HTML books render each chapter through the
//! Normalization → Crossref → Navigation transforms, then pause.
//! [`ChapterCrossrefInventory`] is what each paused chapter contributes at
//! that pause point: its own [`CrossrefIndex`] plus the [`ChapterSeed`] its
//! numbers were produced under. [`aggregate_chapter_inventories`] merges all
//! chapters' inventories into one [`ProjectCrossrefIndex`] — the registry
//! `CrossChapterCrossrefResolveTransform` consults when a chapter references
//! a sibling chapter's target.
//!
//! The aggregation composes each entry's display number itself: the
//! per-chapter index only carries raw `Order` counters (the formatted string
//! is normally produced at display time by `CrossrefRenderTransform`, which
//! has not run for any chapter yet at the pause point). Float/equation
//! numbers come from `format_crossref_number` with the owning chapter's
//! seed; `sec` numbers come from `format_section_number` on the
//! seed-offset section path.
//!
//! These types are serializable so per-chapter inventories can later be
//! persisted (`.quarto/xref/<file-id>.json`) once a freeze-style engine
//! cache exists — the same forward-compatibility contract `CrossrefIndex`
//! already carries.

use hashlink::LinkedHashMap;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::inline::Inlines;
use quarto_source_map::SourceInfo;
use serde::{Deserialize, Serialize};

use crate::crossref::index::{CrossrefIndex, Order};
use crate::crossref::section_number::format_section_number;
use crate::render::ChapterSeed;
use crate::transforms::crossref_render::format_crossref_number;

/// One chapter's harvested crossref inventory, captured at the pause point
/// (immediately after the Navigation phase, before any Finalization work).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterCrossrefInventory {
    /// The chapter's own per-document index. Its `entries` carry every id
    /// the chapter defines, with the raw `Order` counters numbering was
    /// computed from; `max_heading` is needed to format `sec` numbers.
    pub index: CrossrefIndex,

    /// The chapter's seed — needed to compose chapter-scoped display
    /// numbers ("2.3", "A.1") the same way the chapter's own local display
    /// path would. `None` for an unnumbered chapter (`.unnumbered`, which
    /// consumes no slot): its entries then aggregate *flat* (`"1"`, `"2"`),
    /// exactly as the chapter's own display path renders them — matching
    /// the design's "no seed applied at all" rule, never `"0.1"`.
    pub chapter_seed: Option<ChapterSeed>,

    /// Site-root-relative output href of the chapter's rendered file (e.g.
    /// `"ch1.html"`, `"chapters/intro.html"`). Becomes each harvested
    /// entry's [`ProjectCrossrefEntry::owning_chapter_href`].
    pub output_href: String,

    /// Project-relative source path of the chapter (e.g. `"chapters/ch2.qmd"`),
    /// for hub-client's document-based navigation (book-projects P8).
    /// Deliberately distinct from `output_href` (the rendered *output*
    /// path, meaningless to preview) — becomes each harvested entry's
    /// [`ProjectCrossrefEntry::owning_chapter_path`]. `None` for every
    /// real (non-preview) book render; `StaticProjectAnalyzer` is the only
    /// producer that sets it.
    pub owning_chapter_path: Option<String>,
}

/// One cross-chapter target in the project-wide registry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectCrossrefEntry {
    /// Site-root-relative output href of the chapter that defines this id.
    pub owning_chapter_href: String,

    /// Pre-composed display number (e.g. `"2.3"`, `"A.1"`), computed by the
    /// aggregation step from the owning chapter's raw `Order` + seed.
    pub resolved_number: String,

    /// Caption inlines, for link text. `None` when the target has no caption.
    pub caption_inlines: Option<Inlines>,

    /// Ref-type prefix (e.g. `"fig"`, `"tbl"`, `"sec"`).
    pub ref_type: String,

    /// The owning entry's raw order — patched back onto resolved nodes so
    /// display code that reads section paths (the `sec` kind swap between
    /// "Chapter" and "Appendix") keeps working unchanged.
    pub order: Order,

    /// Whether the target sits under an appendix section (as recorded by
    /// the owning chapter's index pass).
    pub in_appendix: bool,

    /// Project-relative source path of the owning chapter (book-projects
    /// P8), copied from [`ChapterCrossrefInventory::owning_chapter_path`].
    /// `None` for every real (non-preview) book render.
    pub owning_chapter_path: Option<String>,
}

/// The project-wide registry: every chapter's crossref targets, keyed by
/// identifier. Built once per book render, after all chapters pause and
/// before any chapter resumes into Finalization.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectCrossrefIndex {
    /// All cross-chapter targets, keyed by identifier (e.g. `"fig-myplot"`).
    /// Insertion order follows book chapter order, so serialize/deserialize
    /// round trips are stable.
    pub entries: LinkedHashMap<String, ProjectCrossrefEntry>,
}

/// Merge every chapter's inventory into the project-wide registry.
///
/// Returns the registry plus any diagnostics produced while merging (a
/// duplicate id across two chapters is diagnosed, and the *first* chapter's
/// definition wins — mirroring the single-document `Q-15-1` policy).
pub fn aggregate_chapter_inventories(
    inventories: &[ChapterCrossrefInventory],
) -> (ProjectCrossrefIndex, Vec<DiagnosticMessage>) {
    aggregate_impl(inventories)
}

fn aggregate_impl(
    inventories: &[ChapterCrossrefInventory],
) -> (ProjectCrossrefIndex, Vec<DiagnosticMessage>) {
    let mut index = ProjectCrossrefIndex::default();
    let mut diagnostics = Vec::new();
    // Source locations of kept entries, so a later collision can point a
    // located detail back at the first definition (Q-15-1's shape).
    let mut first_sources: std::collections::HashMap<String, SourceInfo> =
        std::collections::HashMap::new();

    for inv in inventories {
        for (identifier, entry) in &inv.index.entries {
            if let Some(kept) = index.entries.get(identifier) {
                diagnostics.push(duplicate_across_chapters_diagnostic(
                    identifier,
                    &kept.owning_chapter_href,
                    &inv.output_href,
                    &first_sources[identifier],
                    &entry.source_info,
                ));
                continue;
            }
            let resolved_number = if entry.ref_type == "sec" {
                format_section_number(
                    &entry.order.section,
                    inv.index.max_heading,
                    entry.in_appendix,
                )
            } else {
                // `None` seed (unnumbered chapter) composes flat, matching
                // the chapter's own display path.
                format_crossref_number(entry.order.order, inv.chapter_seed.as_ref())
            };
            index.entries.insert(
                identifier.clone(),
                ProjectCrossrefEntry {
                    owning_chapter_href: inv.output_href.clone(),
                    resolved_number,
                    caption_inlines: entry.caption.clone(),
                    ref_type: entry.ref_type.clone(),
                    order: entry.order.clone(),
                    in_appendix: entry.in_appendix,
                    owning_chapter_path: inv.owning_chapter_path.clone(),
                },
            );
            first_sources.insert(identifier.clone(), entry.source_info.clone());
        }
    }

    (index, diagnostics)
}

/// Build the `Q-15-2` diagnostic for a crossref id defined in more than one
/// chapter of a book. Mirrors the single-document `Q-15-1` diagnostic's
/// shape (`duplicate_id_diagnostic` in `transforms/crossref_index.rs`): the
/// primary location points at the second occurrence; a located detail points
/// back at the first. The aggregation keeps the first chapter's definition.
fn duplicate_across_chapters_diagnostic(
    id: &str,
    first_href: &str,
    second_href: &str,
    first_source: &SourceInfo,
    second_source: &SourceInfo,
) -> DiagnosticMessage {
    DiagnosticMessageBuilder::error("Duplicate crossref identifier across chapters")
        .with_code("Q-15-2")
        .with_location(second_source.clone())
        .problem(format!(
            "The crossref identifier `{id}` is defined in more than one chapter \
             of this book (`{first_href}` and `{second_href}`). A reference like \
             `@{id}` must resolve to exactly one target, so the definition in \
             `{second_href}` is ignored."
        ))
        .add_detail_at("first defined here", first_source.clone())
        .add_hint("Give one of the targets a different `label:` (or `#id`).")
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::index::CrossrefEntry;
    use quarto_error_reporting::DiagnosticKind;
    use quarto_source_map::FileId;

    fn seed(number: u32, is_appendix: bool) -> ChapterSeed {
        ChapterSeed {
            chapter_number: number,
            is_appendix,
        }
    }

    fn entry(
        id: &str,
        ref_type: &str,
        section: Vec<u32>,
        order: u32,
        caption: Option<Inlines>,
    ) -> CrossrefEntry {
        CrossrefEntry {
            identifier: id.to_string(),
            ref_type: ref_type.to_string(),
            parent: None,
            order: Order { section, order },
            caption,
            in_appendix: false,
            source_info: SourceInfo::original(FileId(0), 0, 0),
        }
    }

    fn entry_at_file(
        id: &str,
        ref_type: &str,
        section: Vec<u32>,
        order: u32,
        file: usize,
        offset: usize,
    ) -> CrossrefEntry {
        CrossrefEntry {
            source_info: SourceInfo::original(FileId(file), offset, offset + 1),
            ..entry(id, ref_type, section, order, None)
        }
    }

    fn inlines(text: &str) -> Inlines {
        vec![quarto_pandoc_types::inline::Inline::Str(
            quarto_pandoc_types::inline::Str {
                text: text.to_string(),
                source_info: SourceInfo::original(FileId(0), 0, 0),
            },
        )]
    }

    fn inventory(
        href: &str,
        chapter_seed: Option<ChapterSeed>,
        max_heading: u32,
        entries: Vec<CrossrefEntry>,
    ) -> ChapterCrossrefInventory {
        let mut index = CrossrefIndex::new(FileId(0));
        index.max_heading = max_heading;
        for e in entries {
            index.insert(e);
        }
        ChapterCrossrefInventory {
            index,
            chapter_seed,
            output_href: href.to_string(),
            owning_chapter_path: None,
        }
    }

    #[test]
    fn aggregates_three_chapters_into_a_project_wide_registry() {
        let ch1 = inventory(
            "ch1.html",
            Some(seed(1, false)),
            1,
            vec![
                entry("fig-one", "fig", vec![1], 1, Some(inlines("Dinosaur"))),
                entry("sec-intro", "sec", vec![1], 1, None),
            ],
        );
        let ch2 = inventory(
            "ch2.html",
            Some(seed(2, false)),
            1,
            vec![
                entry("fig-two", "fig", vec![2], 1, Some(inlines("Triceratops"))),
                entry("eq-planck", "eq", vec![2], 1, None),
            ],
        );
        let app = inventory(
            "app-a.html",
            Some(seed(1, true)),
            1,
            vec![entry("fig-app", "fig", vec![1], 1, None)],
        );

        let (index, diags) = aggregate_chapter_inventories(&[ch1, ch2, app]);
        assert!(diags.is_empty(), "distinct ids must produce no diagnostics");

        let one = index.entries.get("fig-one").unwrap();
        assert_eq!(one.owning_chapter_href, "ch1.html");
        assert_eq!(one.resolved_number, "1.1");
        assert_eq!(one.ref_type, "fig");
        assert!(one.caption_inlines.is_some());

        assert_eq!(index.entries.get("fig-two").unwrap().resolved_number, "2.1");
        assert_eq!(
            index.entries.get("fig-two").unwrap().owning_chapter_href,
            "ch2.html"
        );
        assert_eq!(
            index.entries.get("eq-planck").unwrap().resolved_number,
            "2.1"
        );

        // Appendix chapter: the seed's is_appendix drives the letter ("A.1").
        let app_entry = index.entries.get("fig-app").unwrap();
        assert_eq!(app_entry.resolved_number, "A.1");
        assert_eq!(app_entry.owning_chapter_href, "app-a.html");

        // sec: book-scoped via the seed-offset section path; max_heading 1
        // shows only the top component.
        assert_eq!(index.entries.get("sec-intro").unwrap().resolved_number, "1");
    }

    #[test]
    fn sec_entries_format_full_section_paths_in_book_chapters() {
        // Book chapters run with crossref.chapters, forcing max_heading to 1,
        // so the seed-offset top component ("2" for chapter 2) renders.
        let ch2 = inventory(
            "ch2.html",
            Some(seed(2, false)),
            1,
            vec![entry("sec-methods", "sec", vec![2, 1], 1, None)],
        );
        let (index, _) = aggregate_chapter_inventories(&[ch2]);
        assert_eq!(
            index.entries.get("sec-methods").unwrap().resolved_number,
            "2.1"
        );
    }

    #[test]
    fn chapter_and_project_types_round_trip_through_serde() {
        // Pinning test for the "serializable from day one" decision —
        // recorded as passing immediately (the derives carry it); it guards
        // the forward-compatibility contract, it does not drive behavior.
        let ch1 = inventory(
            "ch1.html",
            Some(seed(1, false)),
            1,
            vec![entry(
                "fig-one",
                "fig",
                vec![1],
                1,
                Some(inlines("Dinosaur")),
            )],
        );
        let json = serde_json::to_string(&ch1).unwrap();
        let back: ChapterCrossrefInventory = serde_json::from_str(&json).unwrap();
        assert_eq!(back.output_href, "ch1.html");
        assert_eq!(back.chapter_seed, Some(seed(1, false)));
        assert_eq!(back.index.entries.len(), 1);
        assert_eq!(back.index.max_heading, 1);
        let restored = back.index.entries.get("fig-one").unwrap();
        assert_eq!(restored.order.order, 1);
        assert_eq!(restored.order.section, vec![1]);
        assert_eq!(restored.ref_type, "fig");

        let (index, _) = aggregate_chapter_inventories(&[ch1]);
        let json = serde_json::to_string(&index).unwrap();
        let back: ProjectCrossrefIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(back, index);
    }

    #[test]
    fn duplicate_id_across_chapters_is_diagnosed_and_first_wins() {
        let ch1 = inventory(
            "ch1.html",
            Some(seed(1, false)),
            1,
            vec![entry_at_file("fig-dup", "fig", vec![1], 1, 0, 10)],
        );
        let ch2 = inventory(
            "ch2.html",
            Some(seed(2, false)),
            1,
            vec![entry_at_file("fig-dup", "fig", vec![2], 1, 1, 20)],
        );

        let (index, diags) = aggregate_chapter_inventories(&[ch1, ch2]);
        assert_eq!(diags.len(), 1, "the collision must be diagnosed");

        let diag = &diags[0];
        assert_eq!(diag.code.as_deref(), Some("Q-15-2"));
        assert_eq!(diag.kind, DiagnosticKind::Error);
        assert!(
            diag.location.is_some(),
            "primary location points at the second occurrence"
        );
        assert!(
            diag.details.iter().any(|d| d.location.is_some()),
            "a located detail points back at the first occurrence"
        );
        let text = diag.to_text(None);
        assert!(text.contains("fig-dup"), "text names the id; got: {text}");
        assert!(
            text.contains("ch1.html") && text.contains("ch2.html"),
            "text names both owning chapters; got: {text}"
        );

        // First chapter wins (mirrors Q-15-1's keep-first policy).
        assert_eq!(
            index.entries.get("fig-dup").unwrap().owning_chapter_href,
            "ch1.html"
        );
        assert_eq!(index.entries.len(), 1);
    }

    #[test]
    fn unnumbered_chapter_aggregates_flat_never_zero_point_one() {
        // Design rule: an unnumbered chapter (`.unnumbered`) consumes no
        // chapter slot, so its inventory carries `chapter_seed: None` and
        // its targets aggregate *flat* ("1") — never "0.1" — exactly as the
        // chapter's own local display path renders them.
        let unnumbered = inventory(
            "summary.html",
            None,
            1,
            vec![entry("fig-solo", "fig", vec![1], 1, None)],
        );

        let (index, diags) = aggregate_chapter_inventories(&[unnumbered]);
        assert!(diags.is_empty());

        let solo = index.entries.get("fig-solo").unwrap();
        assert_eq!(
            solo.resolved_number, "1",
            "a None seed must compose flat, never 0.1"
        );
        assert_eq!(solo.owning_chapter_href, "summary.html");
    }
}
