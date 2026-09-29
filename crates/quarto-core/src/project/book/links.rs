/*
 * links.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Cross-chapter link resolution for merged single-file books
 * (book-projects P2). Rust port of the vendored
 * `quarto-pre/book-links.lua` (`index_book_file_targets` +
 * `resolve_book_file_targets`), run once by the merge driver
 * immediately after [`merge_book_chapters`](super::merge) and before
 * the merged document's Crossref phase.
 *
 * Two resolution rules, exactly Q1's:
 *
 * - A relative link whose target contains `#` truncates to its
 *   fragment (`ch1.qmd#sec-foo` → `#sec-foo`) — the target heading is
 *   now in the same document. This fires for *any* relative target
 *   with a hash, chapter file or not (Q1 parity, pinned by
 *   `hash_link_to_non_chapter_file_still_truncates`).
 * - A bare relative file link (`ch1.qmd`, `../ch1.qmd`) resolves to
 *   `#<identifier>` of the target chapter's first level-1 heading,
 *   looked up in the file index the merge's own
 *   `quarto-book-item-file` heading attributes provide. The linking
 *   chapter's `resourceDir` is tracked positionally from the merge's
 *   `<!-- quarto-file-metadata: ... -->` markers, exactly as Q1
 *   tracks `currentFileMetadataState().file.resourceDir`.
 *
 * Once this has run, the vendored `book-links.lua` (gated on the
 * `single-file-book` param) sees only `#...`-shaped or external
 * targets and no-ops — the Lua needs no patch.
 *
 * See `claude-notes/plans/2026-09-21-book-projects-P2-single-file-merge.md`
 * (Decisions, `book-links.lua` bullet) for the full contract.
 */

use hashlink::LinkedHashMap;
use quarto_pandoc_types::Slot;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::{Inline, Inlines};
use quarto_pandoc_types::pandoc::Pandoc;

/// Q1's `isRelativeRef` (`common/paths.lua`): no leading `/`, no
/// `scheme://`, no `data:` prefix, no leading `#`. Fragment-only and
/// external links are left alone entirely.
fn is_relative_ref(target: &str) -> bool {
    !target.starts_with('/')
        && !target.starts_with('#')
        && !target.starts_with("data:")
        && !target
            .find("://")
            .is_some_and(|pos| target[..pos].chars().all(|c| c.is_ascii_alphabetic()) && pos > 0)
}

/// `pandoc.path.normalize`/`flatten` for a project-relative,
/// forward-slashed path: drop `.` and empty segments, pop on `..`.
fn normalize_book_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

const MARKER_PREFIX: &str = "<!-- quarto-file-metadata: ";
const MARKER_SUFFIX: &str = " -->";

/// Decode a `<!-- quarto-file-metadata: base64(json) -->` marker's
/// `resourceDir`, or `None` if the text isn't a marker. Mirrors the
/// merge's own emission (`marker_blocks` in `merge.rs`).
fn marker_resource_dir(text: &str) -> Option<String> {
    let b64 = text
        .strip_prefix(MARKER_PREFIX)
        .and_then(|s| s.strip_suffix(MARKER_SUFFIX))?;
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    json.get("resourceDir")?.as_str().map(|s| s.to_string())
}

/// If this block is one of the merge's two marker shapes
/// (`Paragraph[RawInline(html, …)]` or `RawBlock(html, …)`), its
/// `resourceDir`.
fn block_marker_resource_dir(block: &Block) -> Option<String> {
    match block {
        Block::RawBlock(r) if r.format == "html" => marker_resource_dir(&r.text),
        Block::Paragraph(p) => match p.content.as_slice() {
            [Inline::RawInline(r)] if r.format == "html" => marker_resource_dir(&r.text),
            _ => None,
        },
        _ => None,
    }
}

/// The `file → level-1-heading-identifier` index Q1 builds in
/// `index_book_file_targets`, sourced from the merge's
/// `quarto-book-item-file` heading attributes (first heading per file
/// wins, as in Q1).
fn index_file_targets(blocks: &[Block], map: &mut LinkedHashMap<String, String>) {
    for block in blocks {
        match block {
            Block::Header(h) if h.level == 1 => {
                if let Some(file) = h.attr.2.get("quarto-book-item-file") {
                    map.entry(file.clone()).or_insert_with(|| h.attr.0.clone());
                }
            }
            Block::Div(d) => index_file_targets(&d.content, map),
            Block::BlockQuote(bq) => index_file_targets(&bq.content, map),
            _ => {}
        }
    }
}

/// Resolve every cross-chapter link in a merged single-file book
/// document in place. Returns the number of links rewritten (useful
/// for trace output and for proving the pass is load-bearing).
pub fn resolve_cross_chapter_links(ast: &mut Pandoc) -> usize {
    let mut file_targets = LinkedHashMap::new();
    index_file_targets(&ast.blocks, &mut file_targets);

    let mut resolver = LinkResolver {
        file_targets: &file_targets,
        current_resource_dir: None,
        resolved: 0,
    };
    for block in &mut ast.blocks {
        resolver.visit_block(block);
    }
    resolver.resolved
}

struct LinkResolver<'a> {
    file_targets: &'a LinkedHashMap<String, String>,
    /// The linking chapter's `resourceDir`, tracked positionally from
    /// the merge's markers (Q1's `currentFileMetadataState().file
    /// .resourceDir`). `None` before the first marker.
    current_resource_dir: Option<String>,
    resolved: usize,
}

impl<'a> LinkResolver<'a> {
    fn resolve_link_target(&mut self, target: &mut String) {
        if !is_relative_ref(target) {
            return;
        }
        // Join with the linking chapter's resourceDir and normalize
        // (Q1: `pandoc.path.normalize(flatten(fullPath))`), then:
        let normalized = if target.is_empty() {
            target.clone()
        } else {
            let joined = match &self.current_resource_dir {
                Some(dir) => format!("{dir}/{target}"),
                None => target.clone(),
            };
            normalize_book_path(&joined)
        };
        if let Some(hash_pos) = normalized.find('#') {
            // …a target with a hash truncates to its fragment;
            if normalized[hash_pos..] != *target {
                *target = normalized[hash_pos..].to_string();
                self.resolved += 1;
            }
        } else {
            // …a bare file link resolves through the file index.
            let key = normalized.replace('\\', "/");
            if let Some(section_id) = self.file_targets.get(&key) {
                *target = format!("#{section_id}");
                self.resolved += 1;
            }
        }
    }

    fn visit_block(&mut self, block: &mut Block) {
        // Markers update the positional resourceDir and carry no links.
        if let Some(dir) = block_marker_resource_dir(block) {
            self.current_resource_dir = Some(dir);
            return;
        }
        match block {
            Block::Plain(p) => self.visit_inlines(&mut p.content),
            Block::Paragraph(p) => self.visit_inlines(&mut p.content),
            Block::LineBlock(lb) => {
                for line in lb.content.iter_mut() {
                    self.visit_inlines(line);
                }
            }
            Block::BlockQuote(bq) => {
                for b in bq.content.iter_mut() {
                    self.visit_block(b);
                }
            }
            Block::OrderedList(ol) => {
                for item in ol.content.iter_mut() {
                    for b in item.iter_mut() {
                        self.visit_block(b);
                    }
                }
            }
            Block::BulletList(bl) => {
                for item in bl.content.iter_mut() {
                    for b in item.iter_mut() {
                        self.visit_block(b);
                    }
                }
            }
            Block::DefinitionList(dl) => {
                for (term, defs) in dl.content.iter_mut() {
                    self.visit_inlines(term);
                    for def in defs.iter_mut() {
                        for b in def.iter_mut() {
                            self.visit_block(b);
                        }
                    }
                }
            }
            Block::Header(h) => self.visit_inlines(&mut h.content),
            Block::Div(d) => {
                for b in d.content.iter_mut() {
                    self.visit_block(b);
                }
            }
            Block::Figure(f) => {
                for b in f.content.iter_mut() {
                    self.visit_block(b);
                }
            }
            Block::Table(t) => {
                if let Some(short) = t.caption.short.as_mut() {
                    self.visit_inlines(short);
                }
                if let Some(long) = t.caption.long.as_mut() {
                    for b in long.iter_mut() {
                        self.visit_block(b);
                    }
                }
                for row in t.head.rows.iter_mut().chain(t.foot.rows.iter_mut()) {
                    for cell in row.cells.iter_mut() {
                        for b in cell.content.iter_mut() {
                            self.visit_block(b);
                        }
                    }
                }
                for body in t.bodies.iter_mut() {
                    for row in body.body.iter_mut() {
                        for cell in row.cells.iter_mut() {
                            for b in cell.content.iter_mut() {
                                self.visit_block(b);
                            }
                        }
                    }
                }
            }
            Block::CaptionBlock(cb) => self.visit_inlines(&mut cb.content),
            Block::Custom(c) => {
                for (_name, slot) in c.slots.iter_mut() {
                    self.visit_slot(slot);
                }
            }
            Block::CodeBlock(_)
            | Block::RawBlock(_)
            | Block::HorizontalRule(_)
            | Block::BlockMetadata(_)
            | Block::NoteDefinitionPara(_)
            | Block::NoteDefinitionFencedBlock(_) => {}
        }
    }

    fn visit_inlines(&mut self, inlines: &mut Inlines) {
        for inline in inlines.iter_mut() {
            self.visit_inline(inline);
        }
    }

    fn visit_inline(&mut self, inline: &mut Inline) {
        match inline {
            Inline::Link(link) => {
                self.visit_inlines(&mut link.content);
                let mut target = link.target.0.clone();
                self.resolve_link_target(&mut target);
                link.target.0 = target;
            }
            Inline::Emph(e) => self.visit_inlines(&mut e.content),
            Inline::Underline(u) => self.visit_inlines(&mut u.content),
            Inline::Strong(s) => self.visit_inlines(&mut s.content),
            Inline::Strikeout(s) => self.visit_inlines(&mut s.content),
            Inline::Superscript(s) => self.visit_inlines(&mut s.content),
            Inline::Subscript(s) => self.visit_inlines(&mut s.content),
            Inline::SmallCaps(s) => self.visit_inlines(&mut s.content),
            Inline::Quoted(q) => self.visit_inlines(&mut q.content),
            Inline::Note(n) => {
                for b in n.content.iter_mut() {
                    self.visit_block(b);
                }
            }
            Inline::Span(s) => self.visit_inlines(&mut s.content),
            Inline::Insert(i) => self.visit_inlines(&mut i.content),
            Inline::Delete(d) => self.visit_inlines(&mut d.content),
            Inline::Highlight(h) => self.visit_inlines(&mut h.content),
            Inline::Custom(c) => {
                for (_name, slot) in c.slots.iter_mut() {
                    self.visit_slot(slot);
                }
            }
            Inline::Str(_)
            | Inline::Cite(_)
            | Inline::Code(_)
            | Inline::Space(_)
            | Inline::SoftBreak(_)
            | Inline::LineBreak(_)
            | Inline::Math(_)
            | Inline::RawInline(_)
            | Inline::Shortcode(_)
            | Inline::NoteReference(_)
            | Inline::Attr(_)
            | Inline::EditComment(_)
            | Inline::Image(_) => {}
        }
    }

    fn visit_slot(&mut self, slot: &mut Slot) {
        match slot {
            Slot::Block(b) => self.visit_block(b),
            Slot::Blocks(bs) => {
                for b in bs.iter_mut() {
                    self.visit_block(b);
                }
            }
            Slot::Inline(i) => self.visit_inline(i),
            Slot::Inlines(is) => self.visit_inlines(is),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::book::merge::merge_book_chapters;
    use crate::project::book::render_item::{BookRenderItem, BookRenderItemKind};
    use quarto_pandoc_types::config_value::{ConfigMapEntry, ConfigValue};
    use quarto_source_map::{By, SourceInfo};
    use std::path::PathBuf;

    fn si() -> SourceInfo {
        SourceInfo::generated(By::programmatic_config())
    }

    fn empty_meta() -> ConfigValue {
        ConfigValue::new_map(Vec::<ConfigMapEntry>::new(), si())
    }

    fn parse_chapter(qmd: &str) -> Pandoc {
        let mut sink: Vec<u8> = Vec::new();
        let (pandoc, _ctx, diags) =
            pampa::readers::qmd::read(qmd.as_bytes(), false, "test.qmd", &mut sink, true, None)
                .unwrap_or_else(|diags| panic!("parse failed: {diags:?}"));
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        pandoc
    }

    fn chapter_item(file: &str, number: u32) -> BookRenderItem {
        BookRenderItem {
            kind: BookRenderItemKind::Chapter,
            depth: 0,
            text: None,
            file: Some(PathBuf::from(file)),
            number: Some(number),
        }
    }

    /// Merge the given (file, qmd) chapters and run the resolver.
    fn merge_and_resolve(chapters: Vec<(&str, &str)>) -> (Pandoc, usize) {
        let chapters: Vec<(BookRenderItem, Pandoc)> = chapters
            .into_iter()
            .enumerate()
            .map(|(i, (file, qmd))| (chapter_item(file, (i + 1) as u32), parse_chapter(qmd)))
            .collect();
        let mut merged = merge_book_chapters(chapters, empty_meta(), None);
        let resolved = resolve_cross_chapter_links(&mut merged);
        (merged, resolved)
    }

    /// Every link target in the document, in document order.
    fn link_targets(blocks: &[Block]) -> Vec<String> {
        let mut out = Vec::new();
        collect_link_targets(blocks, &mut out);
        out
    }

    fn collect_link_targets(blocks: &[Block], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                Block::Paragraph(p) => collect_inline_targets(&p.content, out),
                Block::Plain(p) => collect_inline_targets(&p.content, out),
                Block::Div(d) => collect_link_targets(&d.content, out),
                Block::Header(h) => collect_inline_targets(&h.content, out),
                _ => {}
            }
        }
    }

    fn collect_inline_targets(inlines: &[Inline], out: &mut Vec<String>) {
        for inline in inlines {
            match inline {
                Inline::Link(l) => {
                    out.push(l.target.0.clone());
                    collect_inline_targets(&l.content, out);
                }
                Inline::Emph(e) => collect_inline_targets(&e.content, out),
                Inline::Strong(s) => collect_inline_targets(&s.content, out),
                _ => {}
            }
        }
    }

    #[test]
    fn hash_link_truncates_to_fragment() {
        let (merged, resolved) = merge_and_resolve(vec![
            ("ch1.qmd", "# One {#sec-one}\n\nBody.\n"),
            (
                "ch2.qmd",
                "# Two\n\nSee [the first chapter](ch1.qmd#sec-one).\n",
            ),
        ]);
        assert_eq!(resolved, 1);
        assert_eq!(link_targets(&merged.blocks), vec!["#sec-one"]);
    }

    #[test]
    fn bare_file_link_resolves_to_chapter_heading_id() {
        let (merged, resolved) = merge_and_resolve(vec![
            ("ch1.qmd", "# One\n\nBody.\n"),
            ("ch2.qmd", "# Two\n\nSee [the first chapter](ch1.qmd).\n"),
        ]);
        assert_eq!(resolved, 1);
        // ch1's level-1 heading identifier is the parser-assigned "one".
        assert_eq!(link_targets(&merged.blocks), vec!["#one"]);
    }

    #[test]
    fn bare_file_link_normalizes_against_linking_chapters_resource_dir() {
        // The linking chapter lives in `dir/`; `../ch1.qmd` must join
        // against its resourceDir and normalize before the lookup.
        let (merged, resolved) = merge_and_resolve(vec![
            ("ch1.qmd", "# One\n\nBody.\n"),
            (
                "dir/chap2.qmd",
                "# Two\n\nSee [the first chapter](../ch1.qmd).\n",
            ),
        ]);
        assert_eq!(resolved, 1);
        assert_eq!(link_targets(&merged.blocks), vec!["#one"]);
    }

    #[test]
    fn first_level_one_heading_wins_per_file() {
        // A chapter with two H1s: the file index keeps the first.
        let (merged, resolved) = merge_and_resolve(vec![
            ("ch1.qmd", "# One\n\n# Uno\n\nBody.\n"),
            ("ch2.qmd", "# Two\n\nSee [the first chapter](ch1.qmd).\n"),
        ]);
        assert_eq!(resolved, 1);
        assert_eq!(link_targets(&merged.blocks), vec!["#one"]);
    }

    #[test]
    fn hash_link_to_non_chapter_file_still_truncates() {
        // Q1 parity: the hash rule fires for ANY relative target with a
        // hash, whether or not the file is a book chapter (book-links.lua
        // never checks membership on this path).
        let (merged, resolved) = merge_and_resolve(vec![
            ("ch1.qmd", "# One\n\nBody.\n"),
            ("ch2.qmd", "# Two\n\nSee [the script](script.py#L10).\n"),
        ]);
        assert_eq!(resolved, 1);
        assert_eq!(link_targets(&merged.blocks), vec!["#L10"]);
    }

    #[test]
    fn unresolvable_bare_file_link_is_left_unchanged() {
        let (merged, resolved) = merge_and_resolve(vec![
            ("ch1.qmd", "# One\n\nBody.\n"),
            ("ch2.qmd", "# Two\n\nSee [nowhere](not-a-chapter.qmd).\n"),
        ]);
        assert_eq!(resolved, 0);
        assert_eq!(link_targets(&merged.blocks), vec!["not-a-chapter.qmd"]);
    }

    #[test]
    fn external_absolute_data_and_fragment_links_are_untouched() {
        let (merged, resolved) = merge_and_resolve(vec![
            ("ch1.qmd", "# One\n\nBody.\n"),
            (
                "ch2.qmd",
                "# Two\n\n[a](https://example.com/x#y) [b](#local) [c](/abs#x) [d](data:text/plain,hi)\n",
            ),
        ]);
        assert_eq!(resolved, 0);
        assert_eq!(
            link_targets(&merged.blocks),
            vec![
                "https://example.com/x#y",
                "#local",
                "/abs#x",
                "data:text/plain,hi"
            ]
        );
    }

    #[test]
    fn link_inside_div_and_emph_is_resolved() {
        let (merged, resolved) = merge_and_resolve(vec![
            ("ch1.qmd", "# One\n\nBody.\n"),
            (
                "ch2.qmd",
                "# Two\n\n::: {.note}\nSee *[the first chapter](ch1.qmd#sec-one).*\n:::\n",
            ),
        ]);
        assert_eq!(resolved, 1);
        assert_eq!(link_targets(&merged.blocks), vec!["#sec-one"]);
    }

    #[test]
    fn is_relative_ref_matches_q1_gate() {
        assert!(is_relative_ref("ch1.qmd"));
        assert!(is_relative_ref("dir/ch1.qmd#sec"));
        assert!(is_relative_ref("../ch1.qmd"));
        assert!(!is_relative_ref("#local"));
        assert!(!is_relative_ref("/abs/path"));
        assert!(!is_relative_ref("https://example.com"));
        assert!(!is_relative_ref("data:text/plain,hi"));
    }

    #[test]
    fn normalize_book_path_collapses_dot_segments() {
        assert_eq!(normalize_book_path("./ch1.qmd"), "ch1.qmd");
        assert_eq!(normalize_book_path("dir/../ch1.qmd"), "ch1.qmd");
        assert_eq!(normalize_book_path("a/b/../c.qmd"), "a/c.qmd");
    }
}
