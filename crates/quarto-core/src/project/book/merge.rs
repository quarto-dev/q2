/*
 * merge.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Single-file book chapter merge (book-projects P2). Port of Q1's
 * `mergeExecutedFiles`/`bookItemMetadata` (`book-render.ts`): merges N
 * chapters' post-Normalization `Pandoc` bodies into one document,
 * stamping `quarto-book-item-*` attributes on every heading and emitting
 * the paired `<!-- quarto-file-metadata: base64(json) -->` comment
 * markers that both the tracked `book-numbering.lua` patch and the
 * *unpatched* `orange-book` extension's own filter read via
 * `quarto.doc.file_metadata()`.
 *
 * See `claude-notes/plans/2026-09-21-book-projects-P2-single-file-merge.md`
 * for the pinned marker contract and design.
 */

use hashlink::LinkedHashMap;
use quarto_pandoc_types::{
    AttrSourceInfo, Block, Blocks, ConfigValue, Div, Header, Pandoc, Paragraph, RawBlock,
    RawInline, Str,
};
use quarto_source_map::{By, SourceInfo};

use crate::pandoc_filters::params_codec::encode_params_blob;
use crate::project::book::render_item::{BookRenderItem, BookRenderItemKind};

fn generated_source_info() -> SourceInfo {
    SourceInfo::generated(By::programmatic_config())
}

/// Q1's 4-value `bookItemType` vocabulary (`book-render.ts`'s
/// `bookItemMetadata`) — narrower than [`BookRenderItemKind`]: Q2 splits
/// appendix chapters and the appendix divider into one richer kind, but
/// Q1 types an appendix *chapter* `"chapter"` and only the synthetic
/// divider `"appendix"` (appendix-mode is positional state in Q1, flipped
/// by the divider's own marker).
fn book_item_type_str(item: &BookRenderItem) -> &'static str {
    match item.kind {
        BookRenderItemKind::Index => "index",
        BookRenderItemKind::Part => "part",
        BookRenderItemKind::Appendix if item.file.is_none() => "appendix",
        _ => "chapter",
    }
}

/// Project-relative parent dir of the chapter file, `"."` fallback
/// (including for file-less divider items). Always forward-slashed.
fn resource_dir(item: &BookRenderItem) -> String {
    match &item.file {
        Some(f) => match f.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_string_lossy().replace('\\', "/"),
            _ => ".".to_string(),
        },
        None => ".".to_string(),
    }
}

/// The full 5-field `bookItemMetadata` JSON object, Q1's exact shape:
/// `{resourceDir, bookItemType, bookItemNumber, bookItemFile, bookItemDepth}`,
/// with `bookItemFile` omitted (not merely null) when the item has no file.
fn file_metadata_json(item: &BookRenderItem, resource_dir: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "resourceDir".to_string(),
        serde_json::Value::String(resource_dir.to_string()),
    );
    map.insert(
        "bookItemType".to_string(),
        serde_json::Value::String(book_item_type_str(item).to_string()),
    );
    map.insert(
        "bookItemNumber".to_string(),
        match item.number {
            Some(n) => serde_json::Value::Number(n.into()),
            None => serde_json::Value::Null,
        },
    );
    if let Some(file) = &item.file {
        map.insert(
            "bookItemFile".to_string(),
            serde_json::Value::String(file.to_string_lossy().replace('\\', "/")),
        );
    }
    map.insert(
        "bookItemDepth".to_string(),
        serde_json::Value::Number(item.depth.into()),
    );
    serde_json::Value::Object(map)
}

/// The inline marker's payload: just `{resourceDir}`, not the full
/// 5-field shape — Q1 emits a smaller JSON object for the inline
/// (`Paragraph[RawInline]`) marker than for the block (`RawBlock`) one.
fn resource_dir_only_json(resource_dir: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "resourceDir".to_string(),
        serde_json::Value::String(resource_dir.to_string()),
    );
    serde_json::Value::Object(map)
}

fn file_metadata_marker_text(json: &serde_json::Value) -> String {
    let payload = serde_json::to_string(json).expect("file metadata json always serializes");
    format!(
        "<!-- quarto-file-metadata: {} -->",
        encode_params_blob(&payload)
    )
}

/// The two adjacent marker blocks Q1 emits before an item's content:
/// `Paragraph[RawInline(html, b64({resourceDir}))]`, then
/// `RawBlock(html, b64(full 5-field json))`.
fn marker_blocks(item: &BookRenderItem) -> [Block; 2] {
    let dir = resource_dir(item);
    let inline_text = file_metadata_marker_text(&resource_dir_only_json(&dir));
    let block_text = file_metadata_marker_text(&file_metadata_json(item, &dir));

    let inline_marker = Block::Paragraph(Paragraph {
        content: vec![quarto_pandoc_types::Inline::RawInline(RawInline {
            format: "html".to_string(),
            text: inline_text,
            source_info: generated_source_info(),
        })],
        source_info: generated_source_info(),
    });
    let block_marker = Block::RawBlock(RawBlock {
        format: "html".to_string(),
        text: block_text,
        source_info: generated_source_info(),
    });
    [inline_marker, block_marker]
}

/// The `quarto-book-item-*` attributes stamped on every heading in this
/// item's content — the AST equivalent of Q1's positional
/// `currentFileMetadataState()`, read directly off the heading instead of
/// scanning backward through preceding markers.
fn item_attributes(item: &BookRenderItem) -> Vec<(String, String)> {
    let mut attrs = vec![
        (
            "quarto-book-item-type".to_string(),
            book_item_type_str(item).to_string(),
        ),
        ("quarto-book-item-depth".to_string(), item.depth.to_string()),
    ];
    if let Some(n) = item.number {
        attrs.push(("quarto-book-item-number".to_string(), n.to_string()));
    }
    if let Some(file) = &item.file {
        attrs.push((
            "quarto-book-item-file".to_string(),
            file.to_string_lossy().replace('\\', "/"),
        ));
    }
    // Q1 types an appendix chapter "chapter" and never sets `file.appendix`
    // (book-numbering.lua:111 reads a field Q1 never writes) — this
    // attribute is the fix: it's the only place appendix-chapter-ness is
    // recoverable once the divider's positional marker is gone.
    if item.kind == BookRenderItemKind::Appendix && item.file.is_some() {
        attrs.push(("quarto-book-item-appendix".to_string(), "true".to_string()));
    }
    attrs
}

/// Recursively stamp `attrs` onto every [`Block::Header`] reachable
/// through [`Block::Div`]/[`Block::BlockQuote`] nesting.
fn stamp_headers(blocks: &mut [Block], attrs: &[(String, String)]) {
    for block in blocks {
        match block {
            Block::Header(h) => {
                for (k, v) in attrs {
                    h.attr.2.insert(k.clone(), v.clone());
                }
            }
            Block::Div(d) => stamp_headers(&mut d.content, attrs),
            Block::BlockQuote(bq) => stamp_headers(&mut bq.content, attrs),
            _ => {}
        }
    }
}

/// A part/appendix-divider's own `# {title}` heading, built by parsing it
/// through the real qmd reader (reproduces Q1's markdown round-trip — the
/// appendix divider's `"Appendices {.unnumbered}"` text then yields the
/// `unnumbered` class naturally). Falls back to a plain `Str` heading if
/// parsing yields no `Header` (should not happen for well-formed titles,
/// but the merge must not panic on a pathological one).
fn build_divider_heading(title: &str) -> Block {
    let qmd = format!("# {title}\n");
    let mut sink: Vec<u8> = Vec::new();
    if let Ok((pandoc, _ctx, _diags)) = pampa::readers::qmd::read(
        qmd.as_bytes(),
        false,
        "book-divider.qmd",
        &mut sink,
        true,
        None,
    ) && let Some(header) = pandoc.blocks.into_iter().find_map(|b| match b {
        Block::Header(h) => Some(h),
        _ => None,
    }) {
        return Block::Header(header);
    }
    Block::Header(Header {
        level: 1,
        attr: quarto_pandoc_types::empty_attr(),
        content: vec![quarto_pandoc_types::Inline::Str(Str {
            text: title.to_string(),
            source_info: generated_source_info(),
        })],
        source_info: generated_source_info(),
        attr_source: AttrSourceInfo::empty(),
    })
}

/// A part item (with or without an `href`) or the appendix divider wraps
/// its content in a `.quarto-book-part` div — Q1's own
/// `::: {.quarto-book-part}\n...\n:::` — while an appendix *chapter*
/// (typed `"chapter"` per [`book_item_type_str`]) does not.
fn should_wrap_in_part_div(item: &BookRenderItem) -> bool {
    item.kind == BookRenderItemKind::Part
        || (item.kind == BookRenderItemKind::Appendix && item.file.is_none())
}

const BOOK_TITLE_METADATA_KEYS: [&str; 8] = [
    "title",
    "subtitle",
    "author",
    "date",
    "date-format",
    "abstract",
    "description",
    "doi",
];

/// A value is "falsy" in the JS-truthiness sense Q1's
/// `withBookTitleMetadata` relies on: `null`, `false`, or `""`. Arrays and
/// maps are never falsy here (`as_plain_text`/`as_bool` return `None` for
/// them), matching the plan's "arrays/maps always copied" rule.
fn is_falsy(value: &ConfigValue) -> bool {
    if value.is_null() || value.as_bool() == Some(false) {
        return true;
    }
    value.as_plain_text().is_some_and(|s| s.is_empty())
}

/// Port of `withBookTitleMetadata`: copies `title`/`subtitle`/`author`/
/// `date`/`date-format`/`abstract`/`description`/`doi` from the book
/// config into the merged document's metadata, book config winning
/// unconditionally over any stale base value — except a falsy book value,
/// which is skipped (the base metadata, if any, is left as-is).
fn apply_book_title_metadata(meta: &mut ConfigValue, book: &ConfigValue) {
    for key in BOOK_TITLE_METADATA_KEYS {
        if let Some(value) = book.get(key)
            && !is_falsy(value)
        {
            meta.insert_path(&[key], value.clone());
        }
    }
}

/// Merge N chapters' post-Normalization `(BookRenderItem, Pandoc)` pairs
/// into one document: marker blocks + attribute-stamped body per file
/// item, a `.quarto-book-part`-wrapped divider heading per part/appendix
/// divider, and book-level title metadata copied from `book_config`.
///
/// Infallible and pure — provenance/diagnostics for the chapters
/// themselves are the driver's job, not the merge's; `meta` is the
/// driver-supplied base metadata for the merged document.
pub fn merge_book_chapters(
    chapters: Vec<(BookRenderItem, Pandoc)>,
    mut meta: ConfigValue,
    book_config: Option<&ConfigValue>,
) -> Pandoc {
    if let Some(book) = book_config {
        apply_book_title_metadata(&mut meta, book);
    }

    let mut blocks: Blocks = Vec::new();
    for (item, pandoc) in chapters {
        let attrs = item_attributes(&item);
        let [inline_marker, block_marker] = marker_blocks(&item);

        let mut content: Blocks = vec![inline_marker, block_marker];
        if item.file.is_some() {
            let mut body = pandoc.blocks;
            stamp_headers(&mut body, &attrs);
            content.extend(body);
        } else {
            let title = item.text.as_deref().unwrap_or_default();
            let mut heading = build_divider_heading(title);
            stamp_headers(std::slice::from_mut(&mut heading), &attrs);
            content.push(heading);
        }

        if should_wrap_in_part_div(&item) {
            blocks.push(Block::Div(Div {
                attr: (
                    String::new(),
                    vec!["quarto-book-part".to_string()],
                    LinkedHashMap::new(),
                ),
                content,
                source_info: generated_source_info(),
                attr_source: AttrSourceInfo::empty(),
            }));
        } else {
            blocks.extend(content);
        }
    }

    Pandoc { meta, blocks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use std::path::PathBuf;

    fn si() -> SourceInfo {
        generated_source_info()
    }

    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, si())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: si(),
                    value: v,
                })
                .collect(),
            si(),
        )
    }

    fn empty_meta() -> ConfigValue {
        map(vec![])
    }

    fn parse_chapter(qmd: &str) -> Pandoc {
        let mut sink: Vec<u8> = Vec::new();
        let (pandoc, _ctx, diags) =
            pampa::readers::qmd::read(qmd.as_bytes(), false, "test.qmd", &mut sink, true, None)
                .unwrap_or_else(|diags| panic!("parse failed: {diags:?}"));
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        pandoc
    }

    fn chapter_item(
        kind: BookRenderItemKind,
        depth: u32,
        text: Option<&str>,
        file: Option<&str>,
        number: Option<u32>,
    ) -> BookRenderItem {
        BookRenderItem {
            kind,
            depth,
            text: text.map(|s| s.to_string()),
            file: file.map(PathBuf::from),
            number,
        }
    }

    fn decode_marker(text: &str) -> serde_json::Value {
        let b64 = text
            .strip_prefix("<!-- quarto-file-metadata: ")
            .and_then(|s| s.strip_suffix(" -->"))
            .unwrap_or_else(|| panic!("not a file-metadata marker: {text}"));
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("valid base64");
        serde_json::from_slice(&bytes).expect("valid json")
    }

    fn raw_inline_text(block: &Block) -> &str {
        match block {
            Block::Paragraph(p) => match p.content.as_slice() {
                [quarto_pandoc_types::Inline::RawInline(r)] => &r.text,
                other => panic!("expected single RawInline, got {other:?}"),
            },
            other => panic!("expected Paragraph, got {other:?}"),
        }
    }

    fn raw_block_text(block: &Block) -> &str {
        match block {
            Block::RawBlock(r) => &r.text,
            other => panic!("expected RawBlock, got {other:?}"),
        }
    }

    fn header_attr(block: &Block) -> &LinkedHashMap<String, String> {
        match block {
            Block::Header(h) => &h.attr.2,
            other => panic!("expected Header, got {other:?}"),
        }
    }

    fn inlines_plain_text(inlines: &[quarto_pandoc_types::Inline]) -> String {
        inlines
            .iter()
            .map(|i| match i {
                quarto_pandoc_types::Inline::Str(s) => s.text.clone(),
                quarto_pandoc_types::Inline::Space(_) => " ".to_string(),
                other => panic!("unexpected inline in heading text: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn merge_three_chapters_preserves_order_and_stamps_markers() {
        let ch1 = chapter_item(
            BookRenderItemKind::Chapter,
            0,
            None,
            Some("ch1.qmd"),
            Some(1),
        );
        let ch2 = chapter_item(
            BookRenderItemKind::Chapter,
            0,
            None,
            Some("dir/chap2.qmd"),
            Some(2),
        );
        let ch3 = chapter_item(
            BookRenderItemKind::Chapter,
            0,
            None,
            Some("ch3.qmd"),
            Some(3),
        );
        let refs = chapter_item(
            BookRenderItemKind::References,
            0,
            None,
            Some("refs.qmd"),
            Some(4),
        );

        let chapters = vec![
            (ch1, parse_chapter("# One\n\n## Sub One\n\nBody.\n")),
            (ch2, parse_chapter("# Two\n\nBody.\n")),
            (ch3, parse_chapter("# Three\n\nBody.\n")),
            (refs, parse_chapter("# References\n")),
        ];

        let merged = merge_book_chapters(chapters, empty_meta(), None);

        let headers: Vec<&Block> = merged
            .blocks
            .iter()
            .filter(|b| matches!(b, Block::Header(_)))
            .collect();
        // 4 H1s + the "Sub One" H2 = 5 headers, in document order.
        assert_eq!(headers.len(), 5);

        // ch1's H1: type chapter, number 1, depth 0, file ch1.qmd.
        let h1 = header_attr(headers[0]);
        assert_eq!(
            h1.get("quarto-book-item-type").map(String::as_str),
            Some("chapter")
        );
        assert_eq!(
            h1.get("quarto-book-item-number").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            h1.get("quarto-book-item-depth").map(String::as_str),
            Some("0")
        );
        assert_eq!(
            h1.get("quarto-book-item-file").map(String::as_str),
            Some("ch1.qmd")
        );

        // The nested H2 ("Sub One") gets the SAME chapter's attributes —
        // pins recursive stamping through the chapter's own body, not
        // just its top-level H1.
        let h2 = header_attr(headers[1]);
        assert_eq!(
            h2.get("quarto-book-item-type").map(String::as_str),
            Some("chapter")
        );
        assert_eq!(
            h2.get("quarto-book-item-number").map(String::as_str),
            Some("1")
        );

        // References item: typed "chapter" (Q1's vocabulary has no
        // "references" kind), number 4.
        let href = header_attr(headers[4]);
        assert_eq!(
            href.get("quarto-book-item-type").map(String::as_str),
            Some("chapter")
        );
        assert_eq!(
            href.get("quarto-book-item-number").map(String::as_str),
            Some("4")
        );

        // Markers: exactly 2 markers immediately before each chapter's
        // first body block, in book order. ch1's markers sit at [0, 1],
        // its H1 at [2].
        assert_eq!(
            decode_marker(raw_inline_text(&merged.blocks[0])),
            serde_json::json!({"resourceDir": "."})
        );
        assert_eq!(
            decode_marker(raw_block_text(&merged.blocks[1])),
            serde_json::json!({
                "resourceDir": ".",
                "bookItemType": "chapter",
                "bookItemNumber": 1,
                "bookItemFile": "ch1.qmd",
                "bookItemDepth": 0,
            })
        );
        assert!(matches!(merged.blocks[2], Block::Header(_)));

        // ch1 contributes 5 blocks (marker, marker, H1, H2, body
        // paragraph); ch2's own block marker follows immediately at
        // index 6, and resolves resourceDir "dir" from "dir/chap2.qmd".
        let ch2_json = decode_marker(raw_block_text(&merged.blocks[6]));
        assert_eq!(ch2_json["resourceDir"], serde_json::json!("dir"));
        assert_eq!(ch2_json["bookItemFile"], serde_json::json!("dir/chap2.qmd"));
    }

    #[test]
    fn part_divider_positioned_between_chapter_groups() {
        let ch1 = chapter_item(
            BookRenderItemKind::Chapter,
            0,
            None,
            Some("ch1.qmd"),
            Some(1),
        );
        let part = chapter_item(BookRenderItemKind::Part, 0, Some("Part I"), None, None);
        let ch2 = chapter_item(
            BookRenderItemKind::Chapter,
            1,
            None,
            Some("ch2.qmd"),
            Some(2),
        );

        let chapters = vec![
            (ch1, parse_chapter("# One\n\nBody.\n")),
            (
                part,
                Pandoc {
                    meta: empty_meta(),
                    blocks: vec![],
                },
            ),
            (ch2, parse_chapter("# Two\n\nBody.\n")),
        ];

        let merged = merge_book_chapters(chapters, empty_meta(), None);

        // Exactly one .quarto-book-part Div, positioned after ch1's body
        // and before ch2's markers.
        let div_pos = merged
            .blocks
            .iter()
            .position(
                |b| matches!(b, Block::Div(d) if d.attr.1.iter().any(|c| c == "quarto-book-part")),
            )
            .expect("part divider div present");
        let ch1_header_pos = merged
            .blocks
            .iter()
            .position(|b| matches!(b, Block::Header(_)))
            .unwrap();
        assert!(div_pos > ch1_header_pos);

        let Block::Div(div) = &merged.blocks[div_pos] else {
            unreachable!()
        };
        assert_eq!(div.content.len(), 3);
        assert!(matches!(div.content[0], Block::Paragraph(_)));
        assert!(matches!(div.content[1], Block::RawBlock(_)));
        let Block::Header(h) = &div.content[2] else {
            panic!("expected divider heading");
        };
        assert_eq!(inlines_plain_text(&h.content), "Part I");

        // Divider H1 stamped with type "part".
        assert_eq!(
            h.attr.2.get("quarto-book-item-type").map(String::as_str),
            Some("part")
        );

        // Block marker: type "part", number null, file absent, depth 0.
        let marker = decode_marker(raw_block_text(&div.content[1]));
        assert_eq!(marker["bookItemType"], serde_json::json!("part"));
        assert_eq!(marker["bookItemNumber"], serde_json::Value::Null);
        assert!(marker.get("bookItemFile").is_none());
        assert_eq!(marker["bookItemDepth"], serde_json::json!(0));

        // ch2's own markers/H1 come right after the divider.
        assert!(matches!(merged.blocks[div_pos + 1], Block::Paragraph(_)));
        assert!(matches!(merged.blocks[div_pos + 2], Block::RawBlock(_)));
        assert!(matches!(merged.blocks[div_pos + 3], Block::Header(_)));
    }

    #[test]
    fn book_title_metadata_copies_from_book_config() {
        let base_meta = map(vec![
            ("title", s("Stale Title")),
            ("unrelated-key", s("keep me")),
        ]);
        let book = map(vec![
            ("title", s("Real Title")),
            ("subtitle", s("A Subtitle")),
            ("author", s("Jane Doe")),
            ("date", s("2026-01-01")),
            ("date-format", s("iso")),
            ("abstract", s("An abstract.")),
            ("description", s("A description.")),
            ("doi", s("10.1234/x")),
            ("date-modified", s("should not be copied")),
        ]);

        let merged = merge_book_chapters(vec![], base_meta, Some(&book));

        for (key, expected) in [
            ("title", "Real Title"),
            ("subtitle", "A Subtitle"),
            ("author", "Jane Doe"),
            ("date", "2026-01-01"),
            ("date-format", "iso"),
            ("abstract", "An abstract."),
            ("description", "A description."),
            ("doi", "10.1234/x"),
        ] {
            assert_eq!(
                merged.meta.get(key).and_then(|v| v.as_plain_text()),
                Some(expected.to_string()),
                "key {key} not copied correctly"
            );
        }
        assert_eq!(
            merged
                .meta
                .get("unrelated-key")
                .and_then(|v| v.as_plain_text()),
            Some("keep me".to_string())
        );
        assert!(merged.meta.get("date-modified").is_none());
    }

    #[test]
    fn falsy_book_values_are_skipped() {
        let base_meta = map(vec![("title", s("Keep This Title"))]);
        let book = map(vec![
            (
                "title",
                ConfigValue::new_scalar(yaml_rust2::Yaml::Null, si()),
            ),
            ("subtitle", s("")),
        ]);

        let merged = merge_book_chapters(vec![], base_meta, Some(&book));

        // A null/empty book value must not clobber the base metadata.
        assert_eq!(
            merged.meta.get("title").and_then(|v| v.as_plain_text()),
            Some("Keep This Title".to_string())
        );
        assert!(merged.meta.get("subtitle").is_none());
    }

    #[test]
    fn appendix_divider_and_chapters_get_correct_markers_and_attrs() {
        let ch1 = chapter_item(
            BookRenderItemKind::Chapter,
            0,
            None,
            Some("ch1.qmd"),
            Some(1),
        );
        let divider = chapter_item(
            BookRenderItemKind::Appendix,
            0,
            Some("Appendices {.unnumbered}"),
            None,
            None,
        );
        let app_a = chapter_item(
            BookRenderItemKind::Appendix,
            1,
            None,
            Some("app-a.qmd"),
            Some(1),
        );
        let app_b = chapter_item(
            BookRenderItemKind::Appendix,
            1,
            None,
            Some("app-b.qmd"),
            Some(2),
        );

        let chapters = vec![
            (ch1, parse_chapter("# One\n\nBody.\n")),
            (
                divider,
                Pandoc {
                    meta: empty_meta(),
                    blocks: vec![],
                },
            ),
            (app_a, parse_chapter("# First Appendix\n\nBody.\n")),
            (app_b, parse_chapter("# Second Appendix\n\nBody.\n")),
        ];

        let merged = merge_book_chapters(chapters, empty_meta(), None);

        let headers: Vec<&Block> = merged
            .blocks
            .iter()
            .flat_map(|b| match b {
                Block::Div(d) => d.content.iter().collect::<Vec<_>>(),
                other => vec![other],
            })
            .filter(|b| matches!(b, Block::Header(_)))
            .collect();

        // H1 order: One, Appendices, First Appendix, Second Appendix.
        let texts: Vec<String> = headers
            .iter()
            .map(|b| {
                let Block::Header(h) = b else { unreachable!() };
                inlines_plain_text(&h.content)
            })
            .collect();
        assert_eq!(
            texts,
            vec!["One", "Appendices", "First Appendix", "Second Appendix"]
        );

        // Divider H1 carries the "unnumbered" class (parsed from
        // "Appendices {.unnumbered}") and type "appendix".
        let Block::Header(divider_header) = headers[1] else {
            unreachable!()
        };
        assert!(divider_header.attr.1.iter().any(|c| c == "unnumbered"));
        assert_eq!(
            divider_header
                .attr
                .2
                .get("quarto-book-item-type")
                .map(String::as_str),
            Some("appendix")
        );

        // Appendix chapters: typed "chapter" (Q1 parity) + the
        // book-numbering.lua-patch attribute quarto-book-item-appendix,
        // numbered 1/2 in their own fresh sequence, depth 1.
        for (header, expected_number) in [(headers[2], "1"), (headers[3], "2")] {
            let Block::Header(h) = header else {
                unreachable!()
            };
            assert_eq!(
                h.attr.2.get("quarto-book-item-type").map(String::as_str),
                Some("chapter")
            );
            assert_eq!(
                h.attr
                    .2
                    .get("quarto-book-item-appendix")
                    .map(String::as_str),
                Some("true")
            );
            assert_eq!(
                h.attr.2.get("quarto-book-item-number").map(String::as_str),
                Some(expected_number)
            );
            assert_eq!(
                h.attr.2.get("quarto-book-item-depth").map(String::as_str),
                Some("1")
            );
        }

        // Marker types: divider "appendix"; appendix chapters "chapter",
        // numbers 1, 2 (fresh sequence), depth 1.
        let div_pos = merged
            .blocks
            .iter()
            .position(|b| matches!(b, Block::Div(_)))
            .expect("appendix divider div present");
        let Block::Div(div) = &merged.blocks[div_pos] else {
            unreachable!()
        };
        let divider_marker = decode_marker(raw_block_text(&div.content[1]));
        assert_eq!(
            divider_marker["bookItemType"],
            serde_json::json!("appendix")
        );

        let app_a_marker_pos = div_pos + 2; // right after the divider div
        let app_a_marker = decode_marker(raw_block_text(&merged.blocks[app_a_marker_pos]));
        assert_eq!(app_a_marker["bookItemType"], serde_json::json!("chapter"));
        assert_eq!(app_a_marker["bookItemNumber"], serde_json::json!(1));
        assert_eq!(app_a_marker["bookItemDepth"], serde_json::json!(1));
    }

    #[test]
    fn unnumbered_chapter_omits_attribute_but_marker_number_is_null() {
        let item = chapter_item(
            BookRenderItemKind::Chapter,
            0,
            None,
            Some("preface.qmd"),
            None,
        );
        let merged = merge_book_chapters(
            vec![(item, parse_chapter("# Preface {.unnumbered}\n\nBody.\n"))],
            empty_meta(),
            None,
        );

        let Block::Header(h) = &merged.blocks[2] else {
            panic!("expected header");
        };
        assert!(h.attr.2.get("quarto-book-item-number").is_none());

        let marker = decode_marker(raw_block_text(&merged.blocks[1]));
        assert_eq!(marker["bookItemNumber"], serde_json::Value::Null);
        // Key IS present (JSON.stringify would keep an explicit `null`;
        // only `undefined` values are dropped).
        assert!(marker.as_object().unwrap().contains_key("bookItemNumber"));
    }

    /// book-projects P2 item 86 (data-level half; the compiled-PDF half is
    /// P3's): `merge_book_chapters` preserves the `BookRenderItem` appendix
    /// order exactly — the same order P1 computed for the sidebar — with
    /// no reordering, dropping, or duplication. The appendix titles are
    /// deliberately anti-alphabetical ("Zulu" before "Alpha") so any sort
    /// would flip the observable heading sequence.
    #[test]
    fn merge_preserves_appendix_order_exactly() {
        let index = chapter_item(BookRenderItemKind::Index, 0, None, Some("index.qmd"), None);
        let ch1 = chapter_item(
            BookRenderItemKind::Chapter,
            0,
            None,
            Some("ch1.qmd"),
            Some(1),
        );
        let app_divider = chapter_item(
            BookRenderItemKind::Appendix,
            0,
            Some("Appendices"),
            None,
            None,
        );
        let app_a = chapter_item(
            BookRenderItemKind::Appendix,
            1,
            None,
            Some("app-zulu.qmd"),
            Some(1),
        );
        let app_b = chapter_item(
            BookRenderItemKind::Appendix,
            1,
            None,
            Some("app-alpha.qmd"),
            Some(2),
        );

        let chapters = vec![
            (index, parse_chapter("# Home\n\nWelcome.\n")),
            (ch1, parse_chapter("# One\n\nBody.\n")),
            (
                app_divider,
                Pandoc {
                    meta: empty_meta(),
                    blocks: vec![],
                },
            ),
            (app_a, parse_chapter("# Zulu Appendix\n\nBody.\n")),
            (app_b, parse_chapter("# Alpha Appendix\n\nBody.\n")),
        ];

        let merged = merge_book_chapters(chapters, empty_meta(), None);

        // H1s in document order; the file-less appendix divider's heading
        // nests one level deep inside its `quarto-book-part` Div.
        fn collect_h1s(blocks: &[Block], out: &mut Vec<String>) {
            for b in blocks {
                match b {
                    Block::Header(h) if h.level == 1 => out.push(inlines_plain_text(&h.content)),
                    Block::Div(d) => collect_h1s(&d.content, out),
                    _ => {}
                }
            }
        }
        let mut heading_texts = Vec::new();
        collect_h1s(&merged.blocks, &mut heading_texts);
        assert_eq!(
            heading_texts,
            vec![
                "Home",
                "One",
                "Appendices",
                "Zulu Appendix",
                "Alpha Appendix"
            ],
            "merged H1 order must equal the BookRenderItem order exactly"
        );
    }
}
