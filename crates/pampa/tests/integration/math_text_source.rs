//! Provenance of `Math.text` (`Math.text_source`, bd-ieldbghj).
//!
//! `Math.source_info` spans the whole `$…$` / `$$…$$` node, but `text` is
//! not a plain substring of it once the node spans lines: the reader folds
//! each soft break of inline math to a literal `\n` and strips the
//! block-continuation gutter (`> `, list indentation) from the interior
//! lines of display math. `text_source` records that decode so a consumer
//! that parses `text` further (quarto-math) can map any byte of it back to
//! the `.qmd`.
//!
//! The contract these tests pin:
//!
//! 1. the qmd reader always sets `text_source`;
//! 2. `text_source.length() == text.len()`;
//! 3. every byte of `text` maps (via `map_offset`) to a byte inside the
//!    node span, and to the *identical* source byte unless it is a `\n`
//!    synthesized from a soft break;
//! 4. the mapping survives a JSON round trip (`textS` sidecar);
//! 5. text that did not come from the reader carries no mapping.

use pampa::filter_context::FilterContext;
use pampa::filters::{Filter, FilterReturn, topdown_traverse};
use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::{Math, MathType, Pandoc};
use pampa::{readers, writers};
use quarto_source_map::{FileId, SourceInfo};

struct Parsed {
    source: String,
    doc: Pandoc,
    ctx: ASTContext,
    file: FileId,
}

fn parse(source: &str) -> Parsed {
    let (doc, ctx, diagnostics) = readers::qmd::read(
        source.as_bytes(),
        false,
        "math-text-source.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("fixture parses");
    assert!(
        diagnostics.is_empty(),
        "fixture parsed with diagnostics: {diagnostics:?}"
    );
    let file = ctx.current_file_id();
    Parsed {
        source: source.to_string(),
        doc,
        ctx,
        file,
    }
}

/// Every `Math` inline in document order, wherever it is nested.
fn collect_math(doc: &Pandoc) -> Vec<Math> {
    let mut out = Vec::new();
    {
        let mut filter = Filter::new().with_math(|m, _ctx| {
            out.push(m.clone());
            FilterReturn::Unchanged(m)
        });
        let mut fctx = FilterContext::new();
        topdown_traverse(doc.clone(), &mut filter, &mut fctx);
    }
    out
}

fn only_math(p: &Parsed) -> Math {
    let all = collect_math(&p.doc);
    assert_eq!(all.len(), 1, "expected exactly one Math node, got {all:?}");
    all.into_iter().next().unwrap()
}

/// The invariants every reader-produced `Math` must satisfy. Returns the
/// file byte offset each text byte maps to, for tests that want to assert
/// specific positions on top.
fn check_text_provenance(p: &Parsed, m: &Math) -> Vec<usize> {
    let text_source = m
        .text_source
        .as_ref()
        .unwrap_or_else(|| panic!("reader did not record text_source for {:?}", m.text));
    assert_eq!(
        text_source.length(),
        m.text.len(),
        "text_source length must equal text.len() for {:?}",
        m.text
    );

    let node = m
        .source_info
        .preimage_in(p.file)
        .expect("node span resolves in the parsed file");
    let src = p.source.as_bytes();
    assert!(
        p.source[node.clone()].starts_with('$') && p.source[node.clone()].ends_with('$'),
        "node span should cover the delimiters, got {:?}",
        &p.source[node.clone()]
    );

    let mut mapped = Vec::with_capacity(m.text.len());
    for (i, b) in m.text.bytes().enumerate() {
        let loc = text_source
            .map_offset(i, &p.ctx.source_context)
            .unwrap_or_else(|| panic!("text offset {i} of {:?} does not map", m.text));
        assert_eq!(loc.file_id, p.file);
        let off = loc.location.offset;
        assert!(
            node.contains(&off),
            "text[{i}] of {:?} maps to file offset {off}, outside node span {node:?}",
            m.text
        );
        if src[off] != b {
            assert_eq!(
                b, b'\n',
                "text[{i}] = {:?} of {:?} maps to source byte {:?} at {off}; only a folded \
                 soft break may differ from its source byte",
                b as char, m.text, src[off] as char
            );
        }
        mapped.push(off);
    }
    // Offsets never move backwards: the decode is monotone in the source.
    assert!(
        mapped.windows(2).all(|w| w[0] <= w[1]),
        "text bytes map out of order: {mapped:?}"
    );
    // The exclusive end maps too (consumers slice `text[i..text.len()]`).
    assert!(
        text_source
            .map_offset(m.text.len(), &p.ctx.source_context)
            .is_some(),
        "exclusive end offset must map"
    );
    mapped
}

fn offset_of(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not in {haystack:?}"))
}

// ---------------------------------------------------------------------------
// Inline math
// ---------------------------------------------------------------------------

#[test]
fn inline_single_line_is_a_verbatim_substring() {
    let p = parse("Inline $x^2$ here.\n");
    let m = only_math(&p);
    assert_eq!(m.math_type, MathType::InlineMath);
    assert_eq!(m.text, "x^2");
    let mapped = check_text_provenance(&p, &m);
    let start = offset_of(&p.source, "x^2");
    assert_eq!(mapped, vec![start, start + 1, start + 2]);
    // No decode happened, so the mapping is a single contiguous range, not
    // a Concat.
    let ts = m.text_source.as_ref().unwrap();
    assert!(
        matches!(ts, SourceInfo::Original { .. }),
        "single-line inline math should map as one Original range, got {ts:?}"
    );
    assert_eq!(ts.preimage_in(p.file), Some(start..start + 3));
}

#[test]
fn inline_multi_line_soft_break_maps_to_the_newline() {
    let p = parse("$a +\nb$\n");
    let m = only_math(&p);
    assert_eq!(m.text, "a +\nb");
    let mapped = check_text_provenance(&p, &m);
    // "a +\n" is byte-identical to the source, then `b`.
    assert_eq!(mapped, vec![1, 2, 3, 4, 5]);
}

#[test]
fn inline_multi_line_in_blockquote_drops_the_gutter() {
    let source = "> $a\n> b$\n";
    let p = parse(source);
    let m = only_math(&p);
    assert_eq!(m.text, "a\nb");
    let mapped = check_text_provenance(&p, &m);
    let a = offset_of(source, "a");
    let b = offset_of(source, "b$");
    assert_eq!(mapped[0], a);
    assert_eq!(mapped[2], b, "the byte after the fold maps to the real `b`");
    // The folded `\n` maps to the start of the soft break (the source `\n`),
    // not to the `> ` gutter that was dropped.
    assert_eq!(source.as_bytes()[mapped[1]], b'\n');
    let ts = m.text_source.as_ref().unwrap();
    assert!(
        matches!(ts, SourceInfo::Concat { .. }),
        "a fold must produce a Concat, got {ts:?}"
    );
}

#[test]
fn inline_multi_line_in_list_item_drops_the_indent() {
    let source = "- item $a\n  b$ tail\n";
    let p = parse(source);
    let m = only_math(&p);
    assert_eq!(m.text, "a\nb");
    let mapped = check_text_provenance(&p, &m);
    assert_eq!(mapped[0], offset_of(source, "a\n"));
    assert_eq!(mapped[2], offset_of(source, "b$"));
}

// ---------------------------------------------------------------------------
// Display math
// ---------------------------------------------------------------------------

#[test]
fn display_single_line_is_a_verbatim_substring() {
    let p = parse("$$x^2$$\n");
    let m = only_math(&p);
    assert_eq!(m.math_type, MathType::DisplayMath);
    assert_eq!(m.text, "x^2");
    let mapped = check_text_provenance(&p, &m);
    assert_eq!(mapped, vec![2, 3, 4]);
    let ts = m.text_source.as_ref().unwrap();
    assert!(matches!(ts, SourceInfo::Original { .. }), "got {ts:?}");
}

#[test]
fn display_multi_line_without_gutter_is_verbatim() {
    let source = "$$\n\\frac{a}{b}\n$$\n";
    let p = parse(source);
    let m = only_math(&p);
    assert_eq!(m.text, "\n\\frac{a}{b}\n");
    let mapped = check_text_provenance(&p, &m);
    let expected: Vec<usize> = (2..2 + m.text.len()).collect();
    assert_eq!(mapped, expected);
    let ts = m.text_source.as_ref().unwrap();
    assert!(
        matches!(ts, SourceInfo::Original { .. }),
        "nothing was stripped, so the mapping should stay one range, got {ts:?}"
    );
}

#[test]
fn display_in_blockquote_strips_gutter_with_provenance() {
    let source = "> $$\n> x + y\n> $$\n";
    let p = parse(source);
    let m = only_math(&p);
    assert_eq!(m.text, "\nx + y\n");
    let mapped = check_text_provenance(&p, &m);
    // Every kept byte is the real source byte: `x + y` sits after the
    // stripped `> `.
    let x = offset_of(source, "x + y");
    assert_eq!(&mapped[1..6], &[x, x + 1, x + 2, x + 3, x + 4]);
    let ts = m.text_source.as_ref().unwrap();
    assert!(matches!(ts, SourceInfo::Concat { .. }), "got {ts:?}");
    // The hull covers exactly the body between the delimiters, stripped
    // gutters included: deletions still tile the source.
    let body_start = offset_of(source, "\n> x");
    let body_end = source.rfind("$$").unwrap();
    assert_eq!(ts.preimage_in(p.file), Some(body_start..body_end));
}

#[test]
fn display_in_nested_blockquote_strips_every_level() {
    let source = "> > $$\n> > x\n> > $$\n";
    let p = parse(source);
    let m = only_math(&p);
    assert_eq!(m.text, "\nx\n");
    let mapped = check_text_provenance(&p, &m);
    assert_eq!(mapped[1], offset_of(source, "x\n"));
}

#[test]
fn display_in_list_item_strips_indent_with_provenance() {
    let source = "- $$\n  x\n  $$\n";
    let p = parse(source);
    let m = only_math(&p);
    assert_eq!(m.text, "\nx\n");
    let mapped = check_text_provenance(&p, &m);
    assert_eq!(mapped[1], offset_of(source, "x\n"));
}

#[test]
fn display_with_label_attribute_is_mapped() {
    let source = "$$\nx + y\n$$ {#eq-1}\n";
    let p = parse(source);
    let m = only_math(&p);
    assert_eq!(m.text, "\nx + y\n");
    let mapped = check_text_provenance(&p, &m);
    assert_eq!(mapped[1], offset_of(source, "x + y"));
}

#[test]
fn display_inside_strong_multi_line_is_mapped() {
    // bd-qpa2 territory: an inline construct precedes `$$` on the opening
    // line. Whatever the strip decides, kept bytes must map to themselves.
    let source = "**$$y\nz$$**\n";
    let p = parse(source);
    let m = only_math(&p);
    assert_eq!(m.text, "y\nz");
    let mapped = check_text_provenance(&p, &m);
    assert_eq!(mapped, vec![4, 5, 6]);
}

#[test]
fn several_math_nodes_in_one_paragraph_map_independently() {
    let source = "Let $a$ and $$b\nc$$ and $d$.\n";
    let p = parse(source);
    let all = collect_math(&p.doc);
    assert_eq!(all.len(), 3);
    let firsts: Vec<usize> = all
        .iter()
        .map(|m| check_text_provenance(&p, m)[0])
        .collect();
    assert_eq!(
        firsts,
        vec![
            offset_of(source, "a$"),
            offset_of(source, "b\n"),
            offset_of(source, "d$"),
        ]
    );
}

// ---------------------------------------------------------------------------
// Corpus-wide property
// ---------------------------------------------------------------------------

fn qmd_files_under(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            qmd_files_under(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("qmd") {
            out.push(path);
        }
    }
}

/// Every `Math` node in every in-tree fixture that contains math satisfies
/// the provenance contract. Files that do not parse cleanly are skipped
/// (they are other tests' business), but the sweep must find real math.
#[test]
fn every_in_tree_fixture_math_maps_byte_for_byte() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest.parent().unwrap().parent().unwrap();
    let mut files = Vec::new();
    for rel in [
        "crates/pampa/tests",
        "crates/quarto/tests/smoke-all",
        "crates/quarto-core/tests",
        "docs",
    ] {
        qmd_files_under(&repo.join(rel), &mut files);
    }
    files.sort();

    let mut checked_files = 0usize;
    let mut checked_math = 0usize;
    for path in files {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !source.contains('$') {
            continue;
        }
        let Ok((doc, ctx, diagnostics)) = readers::qmd::read(
            source.as_bytes(),
            false,
            &path.to_string_lossy(),
            &mut std::io::sink(),
            true,
            None,
        ) else {
            continue;
        };
        if !diagnostics.is_empty() {
            continue;
        }
        let file = ctx.current_file_id();
        let p = Parsed {
            source,
            doc,
            ctx,
            file,
        };
        let all = collect_math(&p.doc);
        if all.is_empty() {
            continue;
        }
        checked_files += 1;
        for m in &all {
            // Reader-produced math only; the sweep parses raw files, so every
            // node here came from the reader.
            check_text_provenance(&p, m);
            checked_math += 1;
        }
    }
    // 21 files / 36 nodes when written (2026-09-21); the floor only guards
    // against the sweep silently finding nothing.
    assert!(
        checked_files >= 15 && checked_math >= 30,
        "sweep found too little math: {checked_math} nodes in {checked_files} files"
    );
}

// ---------------------------------------------------------------------------
// Round trips and invalidation
// ---------------------------------------------------------------------------

#[test]
fn text_source_survives_a_json_round_trip() {
    let source = "> $a\n> b$\n";
    let p = parse(source);
    let before = only_math(&p);
    let before_ts = before.text_source.clone().expect("reader sets text_source");

    let mut buf = Vec::new();
    writers::json::write(&p.doc, &p.ctx, &mut buf).expect("json write");
    let json = String::from_utf8(buf.clone()).unwrap();
    assert!(
        json.contains("\"textS\""),
        "writer emits the textS sidecar: {json}"
    );

    let (doc2, _ctx2) = readers::json::read(&mut buf.as_slice()).expect("json read");
    let after = collect_math(&doc2);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].text, "a\nb");
    assert_eq!(
        after[0].text_source.as_ref(),
        Some(&before_ts),
        "text_source must round-trip structurally"
    );
}

#[test]
fn json_without_text_sidecar_reads_as_none() {
    let source = "$x$\n";
    let p = parse(source);
    let mut buf = Vec::new();
    writers::json::write(&p.doc, &p.ctx, &mut buf).expect("json write");
    let json = String::from_utf8(buf).unwrap();
    // Strip the sidecar the way a third-party tool that only knows Pandoc's
    // JSON would: drop the key entirely.
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let stripped = strip_key(v, "textS");
    let bytes = serde_json::to_vec(&stripped).unwrap();
    let (doc2, _ctx2) = readers::json::read(&mut bytes.as_slice()).expect("json read");
    let after = collect_math(&doc2);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].text, "x");
    assert!(
        after[0].text_source.is_none(),
        "no sidecar means no mapping, got {:?}",
        after[0].text_source
    );
}

fn strip_key(v: serde_json::Value, key: &str) -> serde_json::Value {
    match v {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .filter(|(k, _)| k != key)
                .map(|(k, v)| (k, strip_key(v, key)))
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(|v| strip_key(v, key)).collect())
        }
        other => other,
    }
}
